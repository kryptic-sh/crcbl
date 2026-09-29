//! `draw_gen.slang`'s `binMain` on its own, against a host transcription of the
//! linear bucket search it routed survivors with before the `(mesh, mode) →
//! bucket` lookup.
//!
//! The lookup is a speed change and nothing else, so every word the pass
//! writes has to be the walk's: each survivor's route, each bucket's mesh word,
//! its static draw arguments and its mesh-dispatch extents — the y extents
//! being the routed survivors counted per region. The frames `draw_gen`'s tests
//! read back have a handful of buckets and meshes with no hierarchy, so this
//! dispatches the entry point directly over tables it chose: no buckets, one, a
//! few shaped like the renderer's with a key two buckets share, the measured
//! scene's 938 and more, with instances whose level mesh is another mesh table
//! entry, instances whose mesh has no bucket, and every material mode — in
//! each draw mode, whose counting differs. **Every word** of the three written
//! buffers is compared, the ones the pass must leave alone included.

use crate::harness::{Headless, poisoned};
use crate::starts::{EntryPipeline, UNREAD_BINDING_BYTES, bytes_for};
use crcbl::hal::{
    Barriers, BindGroupEntry, BindingResource, BufferBarrier, BufferCopy, BufferDesc, BufferHandle,
    BufferUsage, CommandEncoderDesc, ComputePassDesc, Features, MemoryLocation, ResourceState,
    SubmitInfo,
};
use crcbl::shaders::cull::{
    ENTRY_FACE_SHIFT, ENTRY_INDEX_MASK, ENTRY_OCCLUDED, FACE_COUNT, INSTANCE_SURVIVOR_WORD,
    STATS_WORDS,
};
use crcbl::shaders::draw_gen::{
    DRAW_ARGS_WORDS, DrawMode, EARLY_REGION, FACE_REGION_BASE, MATERIAL_MODES, MESH_ARGS_WORDS,
    NO_BUCKET, Params, TableOffsets, WORKGROUP_SIZE, bucket_mesh_word, face_runs_at, pack_tables,
    runs_words,
};
use crcbl::shaders::level_select::MeshLevels;
use crcbl::shaders::mesh::{GpuInstance, GpuMesh};

/// The probe's ring. Nothing is drawn; the fixture needs an extent.
const EXTENT: (u32, u32) = (64, 48);

/// `DrawGenParams::visible_capacity`: the survivor list, and so how many
/// survivors the pass can route.
const CAPACITY: u32 = 1024;

/// Entries of the mesh table. Past the largest id most bucket tables below
/// name, so some instances' meshes have no bucket at all.
const MESHES: u32 = 1200;

/// Instances in the instance array the survivors index.
const INSTANCES: u32 = 900;

/// What every word of `visible_instances` past the survivors, and every word
/// of `args`, holds before the dispatch — so a word the pass wrote and one it
/// left alone are told apart. `counts_and_mesh_args` starts at zero instead,
/// because the pass counts into it with atomic adds.
const SENTINEL: u32 = 0xABAB_ABAB;

/// SplitMix64's finaliser, so neighbouring keys are unrelated.
fn mix(key: u64) -> u32 {
    let mut z = key.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    u32::try_from(z & 0xFFFF_FFFF).expect("masked to 32 bits")
}

/// The bucket tables every draw mode is run over, as (name, meshes, modes).
fn bucket_tables() -> Vec<(&'static str, Vec<u32>, Vec<u32>)> {
    let scattered = |count: u32, salt: u64| {
        (0..count)
            .map(|bucket| {
                let key = (salt << 32) | u64::from(bucket);
                // A third of the mesh table left without buckets, so the
                // instances naming it route nowhere.
                (
                    mix(key) % (MESHES * 2 / 3),
                    mix(key ^ 0x55) % MATERIAL_MODES,
                )
            })
            .unzip()
    };
    let (many_meshes, many_modes) = scattered(938, 1);
    let (more_meshes, more_modes) = scattered(4099, 2);
    // The renderer's shape: each mesh's level meshes, once per mode.
    let mut shaped_meshes = Vec::new();
    let mut shaped_modes = Vec::new();
    for mode in [0, 1, 3] {
        for mesh in 0..40 {
            shaped_meshes.extend(level_run(mesh));
            shaped_modes.extend(std::iter::repeat_n(mode, level_run(mesh).len()));
        }
    }
    vec![
        ("no buckets", vec![], vec![]),
        ("one bucket", vec![drawn(3)], vec![0]),
        // What mesh 3 draws, in mode 2 twice, and what mesh 5 draws in every
        // mode — meshes a sixteenth of the instances name.
        (
            "a shared key",
            vec![drawn(3), drawn(5), drawn(3), drawn(5), drawn(5), drawn(5)],
            vec![2, 0, 2, 1, 2, 3],
        ),
        ("the renderer's shape", shaped_meshes, shaped_modes),
        ("the measured scene", many_meshes, many_modes),
        ("past the measured scene", more_meshes, more_modes),
    ]
}

/// Mesh `mesh`'s level run: itself, then up to two other mesh table entries,
/// as a DAG's separately decimated levels are.
fn level_run(mesh: u32) -> Vec<u32> {
    let levels = 1 + mix(u64::from(mesh) | 0x7000_0000_0000) % 3;
    (0..levels)
        .map(|level| (mesh + level * 401) % MESHES)
        .collect()
}

/// The level `select_level` answers for mesh `mesh`: its record names no
/// groups, so it is the record's `top_level`, chosen here.
fn top_level(mesh: u32) -> u32 {
    let levels = u32::try_from(level_run(mesh).len()).expect("three levels at most");
    mix(u64::from(mesh) | 0x9000_0000_0000) % levels
}

/// The mesh an instance naming `mesh` draws: its level run at [`top_level`].
fn drawn(mesh: u32) -> u32 {
    level_run(mesh)[top_level(mesh) as usize]
}

/// Every mesh's level record and the level table they index. A record names
/// no groups, so `select_level` answers its `top_level` — a level chosen here,
/// which is how a survivor's level mesh differs from the mesh it names without
/// any selection arithmetic in the comparison.
fn levels() -> (Vec<MeshLevels>, Vec<u32>) {
    let mut records = Vec::new();
    let mut level_meshes = Vec::new();
    for mesh in 0..MESHES {
        records.push(MeshLevels {
            first_level: u32::try_from(level_meshes.len()).expect("a small table"),
            top_level: top_level(mesh),
            ..MeshLevels::FLAT
        });
        level_meshes.extend(level_run(mesh));
    }
    (records, level_meshes)
}

fn meshes() -> Vec<GpuMesh> {
    (0..MESHES)
        .map(|mesh| GpuMesh {
            base_index: mesh * 7 + 1,
            index_count: mesh * 3 + 3,
            ..GpuMesh::default()
        })
        .collect()
}

fn instances() -> Vec<GpuInstance> {
    (0..INSTANCES)
        .map(|index| {
            let key = u64::from(index) | 0x3000_0000_0000;
            // Half over the whole mesh table, half over its first eight
            // meshes, so the small tables' keys are drawn too.
            let meshes = if index % 2 == 0 { MESHES } else { 8 };
            GpuInstance {
                mesh: mix(key) % meshes,
                flags: (mix(key ^ 0x11) % MATERIAL_MODES) << GpuInstance::MATERIAL_MODE_SHIFT,
                ..GpuInstance::default()
            }
        })
        .collect()
}

/// The survivor list's entries, tags and all: an instance index, then face bits
/// and an occlusion verdict, which only some draw modes read.
fn survivors(case: &Case) -> Vec<u32> {
    (0..case.survivors.min(CAPACITY))
        .map(|slot| {
            let key = (u64::from(case.buckets()) << 40) | u64::from(slot);
            let instance = mix(key) % INSTANCES;
            let tags = mix(key ^ 0x22) & !ENTRY_INDEX_MASK;
            instance | tags
        })
        .collect()
}

/// One dispatch's inputs.
struct Case {
    name: &'static str,
    mode: DrawMode,
    bucket_meshes: Vec<u32>,
    bucket_modes: Vec<u32>,
    /// `cull.slang`'s true count, which may exceed [`CAPACITY`]; the pass
    /// clamps it.
    survivors: u32,
}

impl Case {
    fn buckets(&self) -> u32 {
        u32::try_from(self.bucket_meshes.len()).expect("a u32 of buckets")
    }

    fn clusters(&self) -> Vec<u32> {
        (0..self.buckets()).map(|bucket| bucket * 5 + 2).collect()
    }

    fn visible_words(&self) -> usize {
        runs_words(CAPACITY, self.buckets(), self.mode == DrawMode::Faces).expect("a small layout")
            as usize
    }

    fn args_words(&self) -> usize {
        (self.mode.regions() * self.buckets()) as usize * DRAW_ARGS_WORDS
    }

    fn counts_words(&self) -> usize {
        (self.mode.regions() * 4 * self.buckets()) as usize
    }

    /// `draw_gen.slang`'s `mesh_arg_word`.
    fn mesh_arg_word(&self, region: u32, bucket: u32, slot: u32) -> usize {
        let buckets = self.buckets();
        (region * 4 * buckets + buckets + bucket * MESH_ARGS_WORDS as u32 + slot) as usize
    }

    /// `draw_gen.slang`'s `arg_word`.
    fn arg_word(&self, region: u32, bucket: u32, field: usize) -> usize {
        (region * self.buckets() + bucket) as usize * DRAW_ARGS_WORDS + field
    }
}

/// What `binMain` wrote when it walked the bucket table, transcribed: each
/// bucket's static half, then each survivor's level mesh looked for bucket by
/// bucket until mesh and mode both matched, counted where its mode says and
/// routed.
fn linear_bin(case: &Case, visible: &mut [u32], args: &mut [u32], counts: &mut [u32]) {
    let (meshes, instances, (records, level_meshes)) = (meshes(), instances(), levels());
    let clusters = case.clusters();
    for bucket in 0..case.buckets() {
        let mesh_id = case.bucket_meshes[bucket as usize];
        let mesh = meshes[mesh_id as usize];
        visible[bucket_mesh_word(CAPACITY, case.buckets(), bucket) as usize] = mesh_id;
        for region in 0..case.mode.regions() {
            args[case.arg_word(region, bucket, 0)] = mesh.index_count;
            args[case.arg_word(region, bucket, 2)] = mesh.base_index;
            args[case.arg_word(region, bucket, 3)] = 0;
            args[case.arg_word(region, bucket, 4)] = 0;
            counts[case.mesh_arg_word(region, bucket, 0)] = clusters[bucket as usize];
            counts[case.mesh_arg_word(region, bucket, 2)] = 1;
        }
    }
    let survivors = survivors(case);
    for (slot, entry) in survivors.iter().enumerate() {
        let instance = instances[(entry & ENTRY_INDEX_MASK) as usize];
        let record = records[instance.mesh as usize];
        let mesh_id = level_meshes[(record.first_level + record.top_level) as usize];
        let mode = instance.material_mode();
        let mut routed = NO_BUCKET;
        for bucket in 0..case.buckets() {
            if case.bucket_meshes[bucket as usize] != mesh_id
                || case.bucket_modes[bucket as usize] != mode
            {
                continue;
            }
            routed = bucket;
            let mut count = |region| counts[case.mesh_arg_word(region, bucket, 1)] += 1;
            if case.mode == DrawMode::Faces {
                for face in 0..FACE_COUNT as u32 {
                    if (entry >> (ENTRY_FACE_SHIFT + face)) & 1 != 0 {
                        count(FACE_REGION_BASE + face);
                    }
                }
            } else {
                count(0);
                if case.mode == DrawMode::Occlusion && entry & ENTRY_OCCLUDED == 0 {
                    count(EARLY_REGION);
                }
            }
            break;
        }
        visible[CAPACITY as usize + slot] = routed;
    }
}

/// Words as the little-endian bytes a buffer holds.
fn bytes_of(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// Dispatches `binMain` over `case` and reads back `visible_instances`, `args`
/// and `counts_and_mesh_args`, word for word.
fn run(headless: &Headless, entry: &EntryPipeline, case: &Case) -> [Vec<u32>; 3] {
    let device = headless.device.as_ref();
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
    let mut read_only = |label, bytes: &[u8]| {
        // Never smaller than the largest element a binding's structure has:
        // D3D12 refuses a structured view that holds none.
        let size = (bytes.len() as u64).max(UNREAD_BINDING_BYTES);
        let handle = buffer(
            label,
            size,
            BufferUsage::STORAGE,
            MemoryLocation::HostUpload,
        );
        device.write_buffer(handle, 0, bytes).expect("write");
        handle
    };

    let (records, level_meshes) = levels();
    let clusters = case.clusters();
    let packed = pack_tables(
        &case.bucket_meshes,
        &case.bucket_modes,
        &clusters,
        &vec![0; clusters.len()],
        &records,
        &[],
        &level_meshes,
    )
    .expect("a small table");
    let instances = read_only(
        "routes probe instances",
        &instances()
            .iter()
            .flat_map(GpuInstance::to_bytes)
            .collect::<Vec<_>>(),
    );
    let meshes = read_only(
        "routes probe meshes",
        &meshes()
            .iter()
            .flat_map(GpuMesh::to_bytes)
            .collect::<Vec<_>>(),
    );
    let mut stats = vec![0u32; STATS_WORDS as usize];
    stats[INSTANCE_SURVIVOR_WORD as usize] = case.survivors;
    let visible_count = read_only("routes probe visible count", &bytes_of(&stats));
    let tables = read_only("routes probe tables", &packed.bytes);
    let TableOffsets {
        bucket_modes_at,
        bucket_clusters_at,
        mesh_levels_at,
        level_groups_at,
        level_meshes_at,
        bucket_lookup_at,
        ..
    } = packed.offsets;
    let params = buffer(
        "routes probe params",
        crcbl::shaders::draw_gen::PARAMS_SIZE as u64,
        BufferUsage::UNIFORM,
        MemoryLocation::HostUpload,
    );
    device
        .write_buffer(
            params,
            0,
            &Params {
                bucket_count: case.buckets(),
                visible_capacity: CAPACITY,
                group_stride: 1,
                bucket_modes_at,
                bucket_clusters_at,
                mesh_levels_at,
                level_groups_at,
                level_meshes_at,
                mode: case.mode,
                draw_regions: case.mode.regions(),
                face_runs_at: face_runs_at(CAPACITY, case.buckets()),
                bucket_lookup_at,
                ..Params::default()
            }
            .to_bytes(),
        )
        .expect("write");

    let initial = [
        survivors_then_sentinel(case),
        vec![SENTINEL; case.args_words()],
        vec![0; case.counts_words()],
    ];
    let written = BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC;
    let sizes = initial
        .each_ref()
        .map(|words| bytes_for(words.len()).max(UNREAD_BINDING_BYTES));
    let targets = [
        "routes probe visible",
        "routes probe args",
        "routes probe counts",
    ]
    .iter()
    .zip(sizes)
    .map(|(label, size)| buffer(label, size, written, MemoryLocation::DeviceLocal))
    .collect::<Vec<_>>();
    let group_state = buffer(
        "routes probe group state",
        (u64::from(CAPACITY) * 4).max(UNREAD_BINDING_BYTES),
        BufferUsage::STORAGE,
        MemoryLocation::DeviceLocal,
    );
    let total: u64 = sizes.iter().sum();
    let source = buffer(
        "routes probe source",
        total,
        BufferUsage::TRANSFER_SRC,
        MemoryLocation::HostUpload,
    );
    let staging = buffer(
        "routes probe readback",
        total,
        BufferUsage::TRANSFER_DST,
        MemoryLocation::HostReadback,
    );
    let mut source_bytes = Vec::new();
    for (words, size) in initial.iter().zip(sizes) {
        let mut bytes = bytes_of(words);
        bytes.resize(size as usize, 0);
        source_bytes.extend(bytes);
    }
    device
        .write_buffer(source, 0, &source_bytes)
        .expect("write");

    let group = device
        .create_bind_group(&crcbl::hal::BindGroupDesc {
            label: Some("routes probe"),
            layout: entry.layout,
            entries: &[
                params,
                instances,
                meshes,
                visible_count,
                tables,
                targets[0],
                targets[1],
                targets[2],
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
        label: Some("routes probe"),
        queue: headless.queue,
    });
    let barrier = |from, to| -> Vec<BufferBarrier> {
        targets
            .iter()
            .map(|buffer| BufferBarrier {
                buffer: *buffer,
                from,
                to,
                queue_transfer: None,
            })
            .collect()
    };
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
    encoder.pipeline_barrier(&Barriers {
        buffers: &barrier(ResourceState::Undefined, ResourceState::TransferDst),
        ..Barriers::default()
    });
    let mut offset = 0;
    for (target, size) in targets.iter().zip(sizes) {
        copy(encoder.as_mut(), source, offset, *target, 0, size);
        offset += size;
    }
    encoder.pipeline_barrier(&Barriers {
        buffers: &barrier(ResourceState::TransferDst, ResourceState::ShaderReadWrite),
        ..Barriers::default()
    });
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("routes probe"),
        timestamp_writes: None,
    });
    encoder.bind_compute_pipeline(entry.pipeline);
    encoder.bind_group(0, group, &[], entry.pipeline_layout);
    // One invocation per bucket and per survivor slot, as
    // `crcbl::render::DrawGen` sizes it: the larger of the two.
    encoder.dispatch(case.buckets().max(CAPACITY).div_ceil(WORKGROUP_SIZE), 1, 1);
    encoder.end_compute_pass();
    encoder.pipeline_barrier(&Barriers {
        buffers: &barrier(ResourceState::ShaderReadWrite, ResourceState::TransferSrc),
        ..Barriers::default()
    });
    let mut offset = 0;
    for (target, size) in targets.iter().zip(sizes) {
        copy(encoder.as_mut(), *target, 0, staging, offset, size);
        offset += size;
    }
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    let mut bytes = poisoned(total as usize);
    headless.readback(staging, total, &mut bytes);
    device.destroy_bind_group(group);
    for buffer in buffers {
        device.destroy_buffer(buffer);
    }
    let mut at = 0;
    let mut read = [Vec::new(), Vec::new(), Vec::new()];
    for ((out, words), size) in read.iter_mut().zip(&initial).zip(sizes) {
        *out = bytes[at..at + words.len() * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect();
        at += size as usize;
    }
    read
}

/// **Every route, argument and extent `binMain` writes through the lookup is
/// the linear walk's, bit for bit**, in every draw mode over every table —
/// and every word the walk leaves alone is left alone.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn the_looked_up_routes_are_the_linear_walks_word_for_word() {
    let headless = Headless::open_at(EXTENT, Features::empty());
    let entry = EntryPipeline::new(&headless, "routes probe", "binMain", WORKGROUP_SIZE);
    let mut failures = Vec::new();
    let (mut compared, mut routed, mut unrouted, mut dispatches) = (0usize, 0usize, 0usize, 0);
    for mode in [DrawMode::Plain, DrawMode::Occlusion, DrawMode::Faces] {
        for (name, bucket_meshes, bucket_modes) in bucket_tables() {
            // Under the capacity, and past it once so the clamp is compared.
            for survivors in [CAPACITY - 24, CAPACITY + 100] {
                let case = Case {
                    name,
                    mode,
                    bucket_meshes: bucket_meshes.clone(),
                    bucket_modes: bucket_modes.clone(),
                    survivors,
                };
                let written = run(&headless, &entry, &case);
                let mut visible = survivors_then_sentinel(&case);
                let mut args = vec![SENTINEL; case.args_words()];
                let mut counts = vec![0; case.counts_words()];
                linear_bin(&case, &mut visible, &mut args, &mut counts);
                let routes = &visible[CAPACITY as usize..][..survivors.min(CAPACITY) as usize];
                routed += routes.iter().filter(|route| **route != NO_BUCKET).count();
                unrouted += routes.iter().filter(|route| **route == NO_BUCKET).count();
                for ((what, gpu), walk) in ["visible_instances", "args", "counts_and_mesh_args"]
                    .iter()
                    .zip(&written)
                    .zip([&visible, &args, &counts])
                {
                    assert_eq!(gpu.len(), walk.len());
                    compared += gpu.len();
                    if let Some(at) = gpu.iter().zip(walk).position(|(a, b)| a != b) {
                        let differing = gpu.iter().zip(walk).filter(|(a, b)| a != b).count();
                        failures.push(format!(
                            "{} in {mode:?} with {survivors} survivors: {differing} word(s) of \
                             {what} differ from the walk's, the first at word {at}: the pass \
                             wrote {:#x}, the walk {:#x}",
                            case.name, gpu[at], walk[at]
                        ));
                    }
                }
                dispatches += 1;
            }
        }
    }
    entry.destroy(&headless);
    headless.finish();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // A comparison over survivors that all went nowhere, or all somewhere,
    // would pass a lookup that answered one of the two for everything.
    assert!(
        routed > 0 && unrouted > 0,
        "{routed} routed, {unrouted} not"
    );
    eprintln!(
        "{}: binMain matched the linear walk on {compared} words over {dispatches} dispatches \
         ({routed} survivors routed, {unrouted} to no bucket)",
        crate::SUITE
    );
}

/// `visible_instances` before the dispatch: the survivors, then [`SENTINEL`].
fn survivors_then_sentinel(case: &Case) -> Vec<u32> {
    let mut words = survivors(case);
    words.resize(case.visible_words(), SENTINEL);
    words
}
