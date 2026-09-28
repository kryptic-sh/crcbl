//! `draw_gen.slang`'s `startsMain` on its own, against a host transcription of
//! the serial walk it replaced.
//!
//! The pass is a prefix sum over one workgroup: each invocation sums a
//! contiguous chunk of the (region, bucket) slots, the chunk totals are scanned
//! in workgroup memory, and each invocation then writes its chunk's starts and
//! draw counts. What can go wrong there is arithmetic nobody sees in a picture —
//! a chunk boundary one slot off, a missing barrier, a lane past the table
//! writing where it should not — and the frames `draw_gen`'s tests read back
//! have a handful of buckets, which one chunk covers. So this dispatches the
//! entry point directly, over y extents it chose, at bucket counts either side
//! of every boundary the chunking has, and compares **every word** of both
//! buffers the pass writes with what the serial walk writes: the starts, the
//! draw counts, and every word neither should touch.
//!
//! The extents include values near `u32::MAX`, so a sum that wraps is compared
//! too: the serial walk wrapped, and integer addition wraps the same whichever
//! way it is grouped.

use crate::harness::{Headless, poisoned};
use crcbl::hal::{
    Barriers, BindGroupEntry, BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource,
    BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferUsage, CommandEncoderDesc,
    ComputePassDesc, Features, MemoryLocation, ResourceState, ShaderStages, SubmitInfo,
};
use crcbl::shaders::cull::FACE_COUNT;
use crcbl::shaders::draw_gen::{
    DRAW_ARGS_WORDS, DrawMode, EARLY_REGION, FACE_REGION_BASE, LATE_REGION, MESH_ARGS_WORDS,
    MeshTasksArgs, Params, STARTS_WORKGROUP_SIZE, face_runs_at, run_start_word, runs_at,
    runs_words,
};
use crcbl::shaders::mesh::{INSTANCE_STRIDE, MESH_ENTRY_STRIDE};

/// The probe's ring. Nothing is drawn; the fixture needs an extent.
const EXTENT: (u32, u32) = (64, 48);

/// `DrawGenParams::visible_capacity`: how long each of the survivor, route and
/// run regions is. Small, because nothing is scattered — it only moves where
/// the starts are and where the first run begins.
const CAPACITY: u32 = 16;

/// What every word of both buffers holds before the dispatch.
///
/// Neither zero nor one, so a draw count the pass wrote and one it left alone
/// are told apart, and so is a start it wrote from one it never reached.
const SENTINEL: u32 = 0xABAB_ABAB;

/// The bucket counts every mode is run at.
///
/// Zero and one; either side of the workgroup size, where every invocation
/// first has a slot of its own and then one lane takes two; the face mode's
/// six slots per bucket either side of it; the measured scene's 938; and
/// tables long enough that every chunk is many slots, one ending in a short
/// chunk and both with lanes past the table. The tables have no cap of their
/// own below what `draw_gen::runs_words` lets a `u32` address, and the chunks
/// lengthen to cover any count.
fn bucket_counts() -> [u32; 10] {
    let face_slots = u32::try_from(FACE_COUNT).expect("six faces");
    [
        0,
        1,
        STARTS_WORKGROUP_SIZE / face_slots,
        STARTS_WORKGROUP_SIZE / face_slots + 1,
        STARTS_WORKGROUP_SIZE - 1,
        STARTS_WORKGROUP_SIZE,
        STARTS_WORKGROUP_SIZE + 1,
        938,
        4099,
        STARTS_WORKGROUP_SIZE * 64 + 1,
    ]
}

/// One dispatch's shape: the mode and the table length.
#[derive(Clone, Copy, Debug)]
struct Case {
    mode: DrawMode,
    buckets: u32,
}

impl Case {
    /// Regions the generator's buffers hold.
    fn regions(self) -> u32 {
        self.mode.regions()
    }

    /// The word of `counts_and_mesh_args` holding bucket `bucket`'s draw count
    /// in region `region` — `draw_gen.slang`'s `count_word`.
    fn count_word(self, region: u32, bucket: u32) -> usize {
        (region * 4 * self.buckets + bucket) as usize
    }

    /// The word of `counts_and_mesh_args` holding bucket `bucket`'s mesh
    /// dispatch y extent in region `region` — `draw_gen.slang`'s
    /// `mesh_arg_word` at `MESH_ARG_GROUP_Y`, which is
    /// [`MeshTasksArgs::group_count_y`]'s word.
    fn extent_word(self, region: u32, bucket: u32) -> usize {
        let args = region * 4 * self.buckets + self.buckets + bucket * MESH_ARGS_WORDS as u32;
        args as usize + 1
    }

    /// Words of `visible_instances`.
    fn visible_words(self) -> usize {
        runs_words(CAPACITY, self.buckets, self.mode == DrawMode::Faces).expect("a small layout")
            as usize
    }

    /// Words of `counts_and_mesh_args`: one count and three extent words per
    /// bucket per region.
    fn counts_words(self) -> usize {
        (self.regions() * 4 * self.buckets) as usize
    }

    /// The counts buffer before the dispatch: every draw count and every x and
    /// z extent at [`SENTINEL`], every y extent [`extent`]'s.
    fn counts_before(self) -> Vec<u32> {
        let mut words = vec![SENTINEL; self.counts_words()];
        for region in 0..self.regions() {
            for bucket in 0..self.buckets {
                let args = MeshTasksArgs {
                    group_count_x: SENTINEL,
                    group_count_y: extent(self, region, bucket),
                    group_count_z: SENTINEL,
                };
                let at = self.extent_word(region, bucket) - 1;
                for (slot, word) in args.to_bytes().as_chunks::<4>().0.iter().enumerate() {
                    words[at + slot] = u32::from_le_bytes(*word);
                }
            }
        }
        words
    }
}

/// Bucket `bucket`'s y extent in region `region`: mostly small, a quarter zero
/// so empty buckets are among them, and now and then close to `u32::MAX` so the
/// sums wrap.
///
/// SplitMix64's finaliser over the slot, so neighbouring slots are unrelated
/// and a start that took the wrong neighbour's extent is a different number.
fn extent(case: Case, region: u32, bucket: u32) -> u32 {
    let mut z = (u64::from(case.buckets) << 40)
        ^ (u64::from(case.mode.word()) << 32)
        ^ (u64::from(region) << 24)
        ^ u64::from(bucket);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let low = u32::try_from(z & 0xFFFF_FFFF).expect("masked to 32 bits");
    match low % 64 {
        0..16 => 0,
        16 => u32::MAX - (low >> 8),
        _ => low % 50,
    }
}

/// What the serial `startsMain` wrote, transcribed: one walk in bucket order —
/// face by face under [`DrawMode::Faces`] — writing each start as the sum in
/// front of it, and a one over each draw count whose extent is not zero.
fn serial_starts(case: Case, visible: &mut [u32], counts: &mut [u32]) {
    let (buckets, extents) = (case.buckets, counts.to_vec());
    let extent_of = |region, bucket| extents[case.extent_word(region, bucket)];
    let start_word = |region, bucket| run_start_word(CAPACITY, buckets, region, bucket) as usize;
    if case.mode == DrawMode::Faces {
        let mut start = face_runs_at(CAPACITY, buckets);
        for face in 0..FACE_COUNT as u32 {
            let region = FACE_REGION_BASE + face;
            for bucket in 0..buckets {
                visible[start_word(region, bucket)] = start;
                let reached = extent_of(region, bucket);
                if reached != 0 {
                    counts[case.count_word(region, bucket)] = 1;
                }
                start = start.wrapping_add(reached);
            }
        }
        return;
    }
    let mut start = runs_at(CAPACITY);
    for bucket in 0..buckets {
        visible[start_word(0, bucket)] = start;
        let routed = extent_of(0, bucket);
        if routed != 0 {
            counts[case.count_word(0, bucket)] = 1;
        }
        if case.mode == DrawMode::Occlusion {
            let early = extent_of(EARLY_REGION, bucket);
            visible[start_word(EARLY_REGION, bucket)] = start;
            visible[start_word(LATE_REGION, bucket)] = start.wrapping_add(early);
            if early != 0 {
                counts[case.count_word(EARLY_REGION, bucket)] = 1;
            }
        }
        start = start.wrapping_add(routed);
    }
}

/// Bytes of a buffer holding `words` words, and never zero: a table with no
/// buckets still binds a buffer, and no backend creates an empty one.
fn bytes_for(words: usize) -> u64 {
    (words.max(1) * 4) as u64
}

/// `startsMain`'s pipeline over `draw_gen.slang`'s nine bindings, laid out as
/// `crcbl::render::DrawGen` lays them out.
struct StartsProbe {
    layout: crcbl::hal::BindGroupLayoutHandle,
    pipeline_layout: crcbl::hal::PipelineLayoutHandle,
    pipeline: crcbl::hal::ComputePipelineHandle,
}

impl StartsProbe {
    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();
        let storage = |binding, read_only, stride: usize| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::COMPUTE,
            kind: BindingKind::StorageBuffer {
                read_only,
                dynamic: false,
                stride: u32::try_from(stride).expect("a small stride"),
            },
            count: 1,
            flags: BindingFlags::empty(),
        };
        let word = size_of::<u32>();
        let entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            storage(1, true, INSTANCE_STRIDE),
            storage(2, true, MESH_ENTRY_STRIDE),
            storage(3, true, word),
            storage(4, true, word),
            storage(5, false, word),
            storage(6, false, word),
            storage(7, false, word),
            storage(8, false, word),
        ];
        let layout = device
            .create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
                label: Some("starts probe"),
                entries: &entries,
            })
            .expect("the draw-args layout");
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
                label: Some("starts probe"),
                bind_group_layouts: &[layout],
                push_constants: None,
            })
            .expect("a pipeline layout");
        let shader = &crcbl::shaders::DRAW_GEN;
        let module = device
            .create_shader_module(&crcbl::hal::ShaderModuleDesc {
                label: Some("draw_gen.slang"),
                spirv: shader.spirv(),
                wgsl: shader.wgsl(),
                msl: shader.msl(),
                dxil: &shader.dxil_containers(),
            })
            .expect("the committed artifacts are accepted");
        let pipeline = device
            .create_compute_pipeline(&crcbl::hal::ComputePipelineDesc {
                label: Some("starts probe"),
                layout: pipeline_layout,
                compute: crcbl::hal::ShaderEntry {
                    module,
                    entry_point: "startsMain",
                },
                workgroup_size: [STARTS_WORKGROUP_SIZE, 1, 1],
            })
            .expect("a compute pipeline");
        device.destroy_shader_module(module);
        Self {
            layout,
            pipeline_layout,
            pipeline,
        }
    }

    /// Fills both written buffers, dispatches the one workgroup, and reads both
    /// back: `visible_instances` and `counts_and_mesh_args`, word for word.
    fn run(&self, headless: &Headless, case: Case) -> (Vec<u32>, Vec<u32>) {
        let device = headless.device.as_ref();
        let visible_bytes = bytes_for(case.visible_words());
        let counts_bytes = bytes_for(case.counts_words());
        let mut buffers = Vec::new();
        let mut buffer = |label: &str, size: u64, usage, memory| {
            let handle = device
                .create_buffer(&BufferDesc {
                    label: Some(label),
                    size,
                    usage,
                    memory,
                })
                .expect("a buffer");
            buffers.push(handle);
            handle
        };
        let mut read = |label| {
            buffer(
                label,
                4 * 32,
                BufferUsage::STORAGE,
                MemoryLocation::HostUpload,
            )
        };
        let (instances, meshes, visible_count, tables) = (
            read("starts probe instances"),
            read("starts probe meshes"),
            read("starts probe visible count"),
            read("starts probe tables"),
        );
        let params = buffer(
            "starts probe params",
            crcbl::shaders::draw_gen::PARAMS_SIZE as u64,
            BufferUsage::UNIFORM,
            MemoryLocation::HostUpload,
        );
        let written = BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC;
        let visible = buffer(
            "starts probe visible",
            visible_bytes,
            written,
            MemoryLocation::DeviceLocal,
        );
        let counts = buffer(
            "starts probe counts",
            counts_bytes,
            written,
            MemoryLocation::DeviceLocal,
        );
        // Bound because the layout names them; `startsMain` reads neither.
        let args = buffer(
            "starts probe args",
            bytes_for(case.regions() as usize * case.buckets as usize * DRAW_ARGS_WORDS),
            BufferUsage::STORAGE,
            MemoryLocation::DeviceLocal,
        );
        let group_state = buffer(
            "starts probe group state",
            4,
            BufferUsage::STORAGE,
            MemoryLocation::DeviceLocal,
        );
        let source = buffer(
            "starts probe source",
            visible_bytes + counts_bytes,
            BufferUsage::TRANSFER_SRC,
            MemoryLocation::HostUpload,
        );
        let staging = buffer(
            "starts probe readback",
            visible_bytes + counts_bytes,
            BufferUsage::TRANSFER_DST,
            MemoryLocation::HostReadback,
        );

        device
            .write_buffer(
                params,
                0,
                &Params {
                    bucket_count: case.buckets,
                    visible_capacity: CAPACITY,
                    mode: case.mode,
                    draw_regions: case.regions(),
                    face_runs_at: face_runs_at(CAPACITY, case.buckets),
                    ..Params::default()
                }
                .to_bytes(),
            )
            .expect("write");
        let mut before = vec![SENTINEL; (visible_bytes / 4) as usize];
        let counts_before = case.counts_before();
        before.extend(&counts_before);
        before.resize(((visible_bytes + counts_bytes) / 4) as usize, SENTINEL);
        let before_bytes: Vec<u8> = before.iter().flat_map(|word| word.to_le_bytes()).collect();
        device
            .write_buffer(source, 0, &before_bytes)
            .expect("write");

        let group = device
            .create_bind_group(&crcbl::hal::BindGroupDesc {
                label: Some("starts probe"),
                layout: self.layout,
                entries: &[
                    params,
                    instances,
                    meshes,
                    visible_count,
                    tables,
                    visible,
                    args,
                    counts,
                    group_state,
                ]
                .iter()
                .zip(0..)
                .map(|(buffer, binding)| BindGroupEntry {
                    binding,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(*buffer),
                })
                .collect::<Vec<_>>(),
                variable_count: None,
            })
            .expect("a bind group");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("starts probe"),
            queue: headless.queue,
        });
        let barrier = |from, to| -> Vec<BufferBarrier> {
            [visible, counts]
                .into_iter()
                .map(|buffer| BufferBarrier {
                    buffer,
                    from,
                    to,
                    queue_transfer: None,
                })
                .collect()
        };
        encoder.pipeline_barrier(&Barriers {
            buffers: &barrier(ResourceState::Undefined, ResourceState::TransferDst),
            ..Barriers::default()
        });
        let copy = |encoder: &mut dyn crcbl::hal::CommandEncoder,
                    src: BufferHandle,
                    src_offset,
                    dst: BufferHandle,
                    dst_offset,
                    size| {
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src,
                src_offset,
                dst,
                dst_offset,
                size,
            });
        };
        copy(encoder.as_mut(), source, 0, visible, 0, visible_bytes);
        copy(
            encoder.as_mut(),
            source,
            visible_bytes,
            counts,
            0,
            counts_bytes,
        );
        encoder.pipeline_barrier(&Barriers {
            buffers: &barrier(ResourceState::TransferDst, ResourceState::ShaderReadWrite),
            ..Barriers::default()
        });
        encoder.begin_compute_pass(&ComputePassDesc {
            label: Some("starts probe"),
            timestamp_writes: None,
        });
        encoder.bind_compute_pipeline(self.pipeline);
        encoder.bind_group(0, group, &[], self.pipeline_layout);
        // One workgroup, as `crcbl::render::DrawGen` dispatches it.
        encoder.dispatch(1, 1, 1);
        encoder.end_compute_pass();
        encoder.pipeline_barrier(&Barriers {
            buffers: &barrier(ResourceState::ShaderReadWrite, ResourceState::TransferSrc),
            ..Barriers::default()
        });
        copy(encoder.as_mut(), visible, 0, staging, 0, visible_bytes);
        copy(
            encoder.as_mut(),
            counts,
            0,
            staging,
            visible_bytes,
            counts_bytes,
        );
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);

        let mut bytes = poisoned((visible_bytes + counts_bytes) as usize);
        headless.readback(staging, visible_bytes + counts_bytes, &mut bytes);
        device.destroy_bind_group(group);
        for buffer in buffers {
            device.destroy_buffer(buffer);
        }
        let words: Vec<u32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect();
        let (visible_words, counts_words) = words.split_at((visible_bytes / 4) as usize);
        (
            visible_words[..case.visible_words()].to_vec(),
            counts_words[..case.counts_words()].to_vec(),
        )
    }

    fn destroy(self, headless: &Headless) {
        let device = headless.device.as_ref();
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
    }
}

/// The first word two buffers disagree at, as a message naming both values.
fn first_difference(what: &str, case: Case, gpu: &[u32], serial: &[u32]) -> Option<String> {
    let at = gpu.iter().zip(serial).position(|(a, b)| a != b)?;
    let differing = gpu.iter().zip(serial).filter(|(a, b)| a != b).count();
    Some(format!(
        "{case:?}: {differing} word(s) of {what} differ from the serial walk's, the first at \
         word {at}: the pass wrote {:#x}, the walk {:#x}",
        gpu[at], serial[at]
    ))
}

/// **Every start and every draw count `startsMain` writes is the serial walk's,
/// bit for bit, in every mode and at every bucket count** — and every word the
/// walk leaves alone is left alone.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn the_parallel_starts_are_the_serial_walks_word_for_word() {
    let headless = Headless::open_at(EXTENT, Features::empty());
    let probe = StartsProbe::new(&headless);
    let mut failures = Vec::new();
    let mut compared = 0usize;
    for mode in [DrawMode::Plain, DrawMode::Occlusion, DrawMode::Faces] {
        for buckets in bucket_counts() {
            let case = Case { mode, buckets };
            let (visible, counts) = probe.run(&headless, case);
            let mut expected_visible = vec![SENTINEL; case.visible_words()];
            let mut expected_counts = case.counts_before();
            serial_starts(case, &mut expected_visible, &mut expected_counts);
            failures.extend(first_difference(
                "visible_instances",
                case,
                &visible,
                &expected_visible,
            ));
            failures.extend(first_difference(
                "counts_and_mesh_args",
                case,
                &counts,
                &expected_counts,
            ));
            assert_eq!(visible.len(), expected_visible.len());
            assert_eq!(counts.len(), expected_counts.len());
            compared += visible.len() + counts.len();
        }
    }
    probe.destroy(&headless);
    headless.finish();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "{}: startsMain matched the serial walk on {compared} words over {} dispatches",
        crate::SUITE,
        3 * bucket_counts().len()
    );
}
