//! The compute path's **Vulkan-specific** rules: the compute scope's record-time
//! refusals, and what `update_bind_group` may do at each descriptor-indexing
//! tier.
//!
//! **The dispatches themselves are no longer here.** "A dispatch really ran and
//! wrote the values it was asked for", and "`dispatch_indirect` read its
//! workgroup count from the buffer at the offset it was named", were the same
//! `ComputeProbe` written four times — here, in `crcbl-wgpu`'s `wgpu_e2e.rs`,
//! and inside `crcbl-mtl`'s and `crcbl-dx12`'s own `src/` — down to the same
//! `PROBE_GROUPS`, the same `PROBE_SENTINEL` and the same decoy arguments at
//! offset zero. They live once now, in `crates/crcbl/tests/hal_seam_e2e.rs`;
//! `CRCBL_GPU=vk crates/crcbl/tests/run-hal-seam-e2e.sh` is where Vulkan runs
//! them. The probe below stays because the two tests that are left need it.
//!
//! What is left is Vulkan's own. `crcbl-hal`'s null recorder records the compute
//! calls and executes none, so a dispatch that did nothing and a dispatch that
//! did the right thing were the same green test there; these two are about the
//! *refusals* around it rather than the dispatch.
//!
//! `update_bind_group_moves_a_dispatch_onto_a_different_buffer` branches on
//! whether the device reports `DESCRIPTOR_INDEXING` and asserts both arms — a
//! rewrite in place on Tier A, a loud refusal on Tier B — and prints which one
//! it took, so a run cannot check less than it looks.

use crate::harness::{Headless, poisoned};
use crcbl_hal::{
    Barriers, BufferDesc, BufferUsage, CommandEncoderDesc, Device, Features, MemoryLocation,
    ResourceState, SubmitInfo,
};

/// Workgroups the probe's buffers are sized for.
///
/// Eight, so `dispatch_indirect` can ask for two and leave six workgroups' worth
/// of untouched sentinel behind it — which is what tells "the argument buffer
/// was read" apart from "everything was dispatched anyway".
const PROBE_GROUPS: u32 = 8;

/// Elements the probe transforms.
const PROBE_ELEMENTS: u32 = PROBE_GROUPS * crcbl_shaders::compute_probe::WORKGROUP_SIZE;

/// What the destination buffer holds before every dispatch.
///
/// Deliberately not zero, and deliberately not a square: a destination that was
/// never written must not be confusable with one the shader wrote, and zero is
/// both its own square and what fresh device memory tends to be.
const PROBE_SENTINEL: u32 = 0xDEAD_BEEF;

/// The probe's input, one distinct value per index.
///
/// Distinct matters: with a constant input, a shader that indexed `source`
/// wrongly would still produce the right number in every slot. `index + 1`
/// avoids zero, whose square is itself.
fn probe_source() -> Vec<u32> {
    (0..PROBE_ELEMENTS).map(|index| index + 1).collect()
}

/// What the destination must hold for `elements` dispatched elements, and the
/// sentinel beyond them.
///
/// Written out here rather than derived from the shader: squaring is a closed
/// form the test states for itself, which is the whole reason the probe squares.
fn probe_expected(elements: u32) -> Vec<u32> {
    probe_source()
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            if (index as u32) < elements {
                value * value
            } else {
                PROBE_SENTINEL
            }
        })
        .collect()
}

/// Everything one compute dispatch needs, built through the seam.
struct ComputeProbe {
    params: crcbl_hal::BufferHandle,
    source: crcbl_hal::BufferHandle,
    destination: crcbl_hal::BufferHandle,
    /// Host-readable copy target, so the result can be asserted rather than
    /// assumed.
    staging: crcbl_hal::BufferHandle,
    /// A host-upload buffer holding [`PROBE_SENTINEL`] in every word, copied
    /// over the destination before each run.
    ///
    /// **This is what a valued `fill_buffer` used to do.** The seam's fill
    /// became a zero-only clear — see `CommandEncoder::clear_buffer` — and
    /// `0xDEAD_BEEF` is the whole point of the prime: an untouched slot has to
    /// hold something no dispatch would ever produce, which zero is not.
    /// Priming through a copy is what `hal_seam_e2e`'s clear exercise does for
    /// the same reason.
    sentinel: crcbl_hal::BufferHandle,
    bind_group_layout: crcbl_hal::BindGroupLayoutHandle,
    bind_group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    pipeline: crcbl_hal::ComputePipelineHandle,
}

/// Bytes one probe buffer occupies.
const fn probe_bytes() -> u64 {
    PROBE_ELEMENTS as u64 * 4
}

impl ComputeProbe {
    /// Builds the pipeline and stages the input in.
    ///
    /// `flags` is the binding-flag set for the destination binding: empty for
    /// the ordinary path, `UPDATE_AFTER_BIND` for the one test that rebinds it
    /// after the group exists. Everything else is identical, which is why they
    /// share this.
    fn new(headless: &Headless, flags: crcbl_hal::BindingFlags) -> Self {
        let device = headless.device.as_ref();
        let params = crcbl_shaders::compute_probe::Params {
            count: PROBE_ELEMENTS,
        }
        .to_bytes();
        let source_bytes: Vec<u8> = probe_source()
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let sentinel_bytes: Vec<u8> = std::iter::repeat_n(
            PROBE_SENTINEL.to_le_bytes(),
            probe_bytes() as usize / size_of::<u32>(),
        )
        .flatten()
        .collect();

        let upload = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe upload"),
                size: (params.len() + source_bytes.len()) as u64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        device.write_buffer(upload, 0, &params).expect("write");
        device
            .write_buffer(upload, params.len() as u64, &source_bytes)
            .expect("write");

        // Its own buffer rather than a tail on `upload`, because `upload` is
        // destroyed at the end of this function and every later run needs this
        // one.
        let sentinel = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe sentinel"),
                size: probe_bytes(),
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a sentinel buffer");
        device
            .write_buffer(sentinel, 0, &sentinel_bytes)
            .expect("write");

        let params_buffer = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe params"),
                size: crcbl_shaders::compute_probe::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a uniform buffer");
        let source = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe source"),
                size: probe_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a source buffer");
        let destination = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe destination"),
                size: probe_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a destination buffer");
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe readback"),
                size: probe_bytes(),
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("compute probe upload"),
            queue: headless.queue,
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: upload,
            src_offset: 0,
            dst: params_buffer,
            dst_offset: 0,
            size: params.len() as u64,
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: upload,
            src_offset: params.len() as u64,
            dst: source,
            dst_offset: 0,
            size: probe_bytes(),
        });
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                crcbl_hal::BufferBarrier {
                    buffer: params_buffer,
                    from: ResourceState::TransferDst,
                    to: ResourceState::ShaderRead,
                    queue_transfer: None,
                },
                crcbl_hal::BufferBarrier {
                    buffer: source,
                    from: ResourceState::TransferDst,
                    to: ResourceState::ShaderRead,
                    queue_transfer: None,
                },
            ],
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
        device.destroy_buffer(upload);

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
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags,
            },
        ];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("compute probe"),
                entries: &layout_entries,
            })
            .expect("the probe's layout");

        let group_entries = [
            crcbl_hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(params_buffer),
            },
            crcbl_hal::BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(source),
            },
            crcbl_hal::BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(destination),
            },
        ];
        let bind_group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("compute probe"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
                label: Some("compute probe"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&crcbl_hal::ShaderModuleDesc {
                label: Some("compute_probe.slang"),
                spirv: crcbl_shaders::COMPUTE_PROBE.spirv(),
                wgsl: crcbl_shaders::COMPUTE_PROBE.wgsl(),
                msl: crcbl_shaders::COMPUTE_PROBE.msl(),
                dxil: &[],
            })
            .expect("the committed SPIR-V is accepted");
        // The manifest's name rather than a literal: it is read out of the
        // artifact's `OpEntryPoint` by the compile script, so a Slang release
        // that renamed it would fail here rather than in a driver.
        let entry_point = crcbl_shaders::COMPUTE_PROBE
            .entry_point(crcbl_shaders::Stage::Compute)
            .expect("the probe has exactly one compute entry point");
        let pipeline = device
            .create_compute_pipeline(&crcbl_hal::ComputePipelineDesc {
                label: Some("compute probe"),
                layout: pipeline_layout,
                compute: crcbl_hal::ShaderEntry {
                    module,
                    entry_point,
                },
                // The shader's own number, not a literal: `crcbl-shaders`
                // checks this constant against the `[numthreads(…)]` in
                // `compute_probe.slang`, and this backend checks it again
                // against the SPIR-V it is compiling.
                workgroup_size: [crcbl_shaders::compute_probe::WORKGROUP_SIZE, 1, 1],
            })
            .expect("a compute pipeline");
        device.destroy_shader_module(module);

        Self {
            params: params_buffer,
            source,
            destination,
            staging,
            sentinel,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
        }
    }

    /// Fills the destination with the sentinel, runs `record` inside a compute
    /// pass, and reads the destination back.
    ///
    /// `record` is the *only* thing that varies between the dispatch and the
    /// indirect-dispatch tests, so both go through the same barriers and the
    /// same readback and a difference in the result is a difference in the
    /// dispatch.
    fn run(
        &self,
        headless: &Headless,
        record: impl FnOnce(&mut dyn crcbl_hal::CommandEncoder),
    ) -> Vec<u32> {
        self.run_into(headless, self.destination, record)
    }

    /// [`run`](Self::run), against a nominated destination buffer.
    ///
    /// The buffer the *bind group* points at is not necessarily this one — that
    /// is what `update_bind_group` changes — so the sentinel fill and the
    /// readback name it explicitly.
    fn run_into(
        &self,
        headless: &Headless,
        destination: crcbl_hal::BufferHandle,
        record: impl FnOnce(&mut dyn crcbl_hal::CommandEncoder),
    ) -> Vec<u32> {
        let device = headless.device.as_ref();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("compute probe dispatch"),
            queue: headless.queue,
        });
        let buffer_barrier = |buffer, from, to| crcbl_hal::BufferBarrier {
            buffer,
            from,
            to,
            queue_transfer: None,
        };
        // `TransferSrc` as the source state is vacuous on the first run and is
        // the real prior use on every later one — a buffer barrier carries no
        // layout, so naming a wider source scope than happened costs a stage
        // mask and cannot be wrong.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                destination,
                ResourceState::TransferSrc,
                ResourceState::TransferDst,
            )],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: self.sentinel,
            src_offset: 0,
            dst: destination,
            dst_offset: 0,
            size: probe_bytes(),
        });
        // `ShaderReadWrite`, not `ShaderWrite`, even though `compute_probe.slang`
        // only ever assigns to `destination`. A barrier names the access the
        // *descriptor* permits rather than the one the source performs, and a
        // `VK_DESCRIPTOR_TYPE_STORAGE_BUFFER` is read-write unless the SPIR-V
        // marks it `NonReadable` — which Slang does not emit for an
        // `RWStructuredBuffer`. Naming the write alone leaves the fill's
        // transfer write unsynchronised against a read the driver is entitled to
        // make, which is a `SYNC-HAZARD-READ-AFTER-WRITE` the validation layer
        // reports. Measured: caught by CI's layer, missed by this machine's.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                destination,
                ResourceState::TransferDst,
                ResourceState::ShaderReadWrite,
            )],
            ..Barriers::default()
        });

        encoder.begin_compute_pass(&crcbl_hal::ComputePassDesc {
            label: Some("compute probe"),
            timestamp_writes: None,
        });
        encoder.bind_compute_pipeline(self.pipeline);
        // Inside the pass, because the open scope is the only signal the seam
        // gives the backend about which bind point a group is for.
        encoder.bind_group(0, self.bind_group, &[], self.pipeline_layout);
        record(encoder.as_mut());
        encoder.end_compute_pass();

        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                destination,
                ResourceState::ShaderReadWrite,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: destination,
            src_offset: 0,
            dst: self.staging,
            dst_offset: 0,
            size: probe_bytes(),
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);

        self.read_words(headless)
    }

    /// Copies `buffer` to the host and decodes it, with no fill and no dispatch.
    ///
    /// Separate from [`run_into`](Self::run_into) because the interesting
    /// question about a buffer the bind group no longer names is what is *still*
    /// in it — and a helper that filled it first could only ever answer that
    /// with the value it had just written.
    fn read_back(&self, headless: &Headless, buffer: crcbl_hal::BufferHandle) -> Vec<u32> {
        let device = headless.device.as_ref();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("compute probe read"),
            queue: headless.queue,
        });
        encoder.pipeline_barrier(&Barriers {
            buffers: &[crcbl_hal::BufferBarrier {
                buffer,
                from: ResourceState::ShaderReadWrite,
                to: ResourceState::TransferSrc,
                queue_transfer: None,
            }],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: buffer,
            src_offset: 0,
            dst: self.staging,
            dst_offset: 0,
            size: probe_bytes(),
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
        self.read_words(headless)
    }

    /// The staging buffer's contents, as the `u32`s the probe writes.
    fn read_words(&self, headless: &Headless) -> Vec<u32> {
        let mut bytes = poisoned(probe_bytes() as usize);
        headless.readback(self.staging, probe_bytes(), &mut bytes);
        bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect()
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.sentinel);
        device.destroy_buffer(self.staging);
        device.destroy_buffer(self.destination);
        device.destroy_buffer(self.source);
        device.destroy_buffer(self.params);
    }
}

/// Compares a probe result against what the CPU says it should be, and says
/// which element disagreed first.
///
/// The element count is asserted before the values: a readback that came back
/// short would otherwise satisfy a `zip` over nothing at all.
fn assert_probe(actual: &[u32], expected: &[u32], what: &str) {
    assert_eq!(
        actual.len(),
        PROBE_ELEMENTS as usize,
        "{what}: the readback is not the whole destination buffer"
    );
    assert_eq!(expected.len(), actual.len(), "{what}: expectation length");
    if let Some((index, (got, want))) = actual
        .iter()
        .zip(expected)
        .enumerate()
        .find(|(_, (got, want))| got != want)
    {
        panic!(
            "{what}: element {index} is {got} ({got:#x}), expected {want} ({want:#x}). \
             {} of {} elements were expected to be written.",
            expected.iter().filter(|v| **v != PROBE_SENTINEL).count(),
            expected.len()
        );
    }
}

/// The compute scope's own rules, at record time.
///
/// `crcbl-hal`'s null recorder rejects a nested pass and an unclosed one, and
/// the seam says a backend "may assume these hold". `crcbl-vk` does not merely
/// assume: it reports both, and this is where that stops being a claim about
/// code nobody ran. Both encoders fail at `finish`, so neither reaches a queue.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn compute_passes_do_not_nest_and_may_not_be_left_open() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    let mut nested = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("nested compute pass"),
        queue: headless.queue,
    });
    nested.begin_compute_pass(&crcbl_hal::ComputePassDesc {
        label: None,
        timestamp_writes: None,
    });
    nested.begin_compute_pass(&crcbl_hal::ComputePassDesc {
        label: None,
        timestamp_writes: None,
    });
    let error = nested
        .finish()
        .expect_err("a nested compute pass must fail recording, not the driver");
    let crcbl_hal::HalError::InvalidDescriptor(message) = &error else {
        panic!("a nested pass is a descriptor problem: {error}");
    };
    assert!(message.contains("do not nest"), "{message}");

    let mut unclosed = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("unclosed compute pass"),
        queue: headless.queue,
    });
    unclosed.begin_compute_pass(&crcbl_hal::ComputePassDesc {
        label: None,
        timestamp_writes: None,
    });
    let error = unclosed
        .finish()
        .expect_err("a compute pass left open must fail recording");
    let crcbl_hal::HalError::InvalidDescriptor(message) = &error else {
        panic!("an unclosed pass is a descriptor problem: {error}");
    };
    assert!(message.contains("compute pass still open"), "{message}");

    // And a well-formed pass on the same device still records, so the two
    // failures above are the shape of the pass rather than the encoder.
    let mut good = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("well-formed compute pass"),
        queue: headless.queue,
    });
    good.begin_compute_pass(&crcbl_hal::ComputePassDesc {
        label: Some("empty"),
        timestamp_writes: None,
    });
    good.end_compute_pass();
    let commands = good.finish().expect("an empty compute pass records");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    headless.finish();
}

/// `Device::update_bind_group`: the bindless write path, and the one call the
/// seam permits while command buffers referencing the group are still pending.
///
/// Both tiers are covered and both are asserted. With `DESCRIPTOR_INDEXING` the
/// group is rebuilt in place to point at a second destination buffer and the
/// *same* pipeline then writes the new one and leaves the old one alone — which
/// a write that silently did nothing could not produce. Without it, the call
/// must be refused loudly, because rewriting a set that a pending submission may
/// reference is undefined behaviour rather than a slow path.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn update_bind_group_moves_a_dispatch_onto_a_different_buffer() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    let indexing = device
        .caps()
        .features
        .contains(Features::DESCRIPTOR_INDEXING);

    if !indexing {
        // Tier B: the layout cannot even carry the flag, so the refusal is
        // asserted on the plain group the probe already builds.
        let probe = ComputeProbe::new(&headless, crcbl_hal::BindingFlags::empty());
        let error = device
            .update_bind_group(probe.bind_group, &[])
            .expect_err("a set without UPDATE_AFTER_BIND must refuse a rewrite");
        assert!(
            matches!(error, crcbl_hal::HalError::Unsupported { .. }),
            "the refusal must be loud and typed: {error}"
        );
        eprintln!("vk e2e: update_bind_group — Tier B device refused the rewrite: {error}");
        probe.destroy(device);
        headless.finish();
        return;
    }

    eprintln!("vk e2e: update_bind_group — Tier A device, rewriting a live set");
    let probe = ComputeProbe::new(&headless, crcbl_hal::BindingFlags::UPDATE_AFTER_BIND);
    let second = device
        .create_buffer(&BufferDesc {
            label: Some("compute probe second destination"),
            size: probe_bytes(),
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a second destination buffer");

    // Before: the group still names the original buffer.
    let before = probe.run(&headless, |encoder| {
        encoder.dispatch(PROBE_GROUPS, 1, 1);
    });
    assert_probe(
        &before,
        &probe_expected(PROBE_ELEMENTS),
        "before the rewrite",
    );

    device
        .update_bind_group(
            probe.bind_group,
            &[crcbl_hal::BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(second),
            }],
        )
        .expect("a Tier A device rewrites an UPDATE_AFTER_BIND set");

    // Reset the original to the sentinel and dispatch nothing, so the squares
    // step one left in it cannot be mistaken for squares this step wrote. Setup,
    // not an assertion — the assertion is three lines down.
    let reset = probe.run_into(&headless, probe.destination, |_| {});
    assert!(
        reset.iter().all(|value| *value == PROBE_SENTINEL),
        "a compute pass with no dispatch in it must write nothing"
    );

    // After: the *second* buffer is the one the same pipeline fills…
    let after = probe.run_into(&headless, second, |encoder| {
        encoder.dispatch(PROBE_GROUPS, 1, 1);
    });
    assert_probe(&after, &probe_expected(PROBE_ELEMENTS), "after the rewrite");

    // …and the original is untouched by that dispatch, which is what makes this
    // a rebind rather than a second write to the same place. An
    // `update_bind_group` that did nothing would have sent the dispatch above
    // here instead, failing both this and the assertion before it.
    let untouched = probe.read_back(&headless, probe.destination);
    assert!(
        untouched.iter().all(|value| *value == PROBE_SENTINEL),
        "the rebound-away buffer was written after the rewrite; got {:?}…",
        &untouched[..4]
    );

    // The refusal is not a *tier* rule, it is a *layout* rule — so it is checked
    // here too, on the Tier A device, rather than only in the branch above that
    // a machine with a Tier A driver never enters. A group whose layout did not
    // ask for `UPDATE_AFTER_BIND` must be refused whatever the device can do,
    // because a pending command buffer may still reference the set.
    let plain_entries = [crcbl_hal::BindGroupLayoutEntry {
        binding: 0,
        visibility: crcbl_hal::ShaderStages::COMPUTE,
        kind: crcbl_hal::BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        },
        count: 1,
        flags: crcbl_hal::BindingFlags::empty(),
    }];
    let plain_layout = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("no update-after-bind"),
            entries: &plain_entries,
        })
        .expect("a flagless layout works on both tiers");
    let plain_group = device
        .create_bind_group(&crcbl_hal::BindGroupDesc {
            label: Some("no update-after-bind"),
            layout: plain_layout,
            entries: &[crcbl_hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(probe.source),
            }],
            variable_count: None,
        })
        .expect("a bind group");
    let error = device
        .update_bind_group(plain_group, &[])
        .expect_err("a set without UPDATE_AFTER_BIND must refuse a rewrite on any tier");
    assert!(
        matches!(error, crcbl_hal::HalError::Unsupported { .. }),
        "the refusal must be loud and typed: {error}"
    );
    device.destroy_bind_group(plain_group);
    device.destroy_bind_group_layout(plain_layout);

    device.destroy_buffer(second);
    probe.destroy(device);
    headless.finish();
}
