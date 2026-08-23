//! The gate underneath every other module: proof the validation layer is
//! **wired**, not merely quiet.
//!
//! Every other test in this suite ends in `Headless::finish`, which asserts a
//! clean report — and every one of them would pass just as happily against a
//! messenger that was never created, which is the failure mode that turns "zero
//! validation errors" into a slogan. So this module exists to commit deliberate
//! violations and assert the layer noticed: one ordinary specification
//! violation, one synchronisation hazard — a separate opt-in with a separate
//! failure mode, silently doing nothing until P1.2 — and one that is committed
//! by `crcbl-vk` itself, inside a frame loop, when
//! `CRCBL_VK_VALIDATION_PROVOKE` asks for it, which is the only one of the
//! three a *binary* can commit.
//!
//! Both violations are chosen to be caught at **record** time and thrown away
//! rather than submitted. A spec violation is undefined behaviour and a software
//! rasteriser is under no obligation to survive one — an earlier version of the
//! first test used an oversized render area, which segfaults lavapipe, and that
//! is the first place radv and lavapipe disagreed.
//!
//! The sync test also *measures* how far this machine's layer sees across
//! submissions and prints the marker line `tests/run-vk-e2e.sh` turns into a
//! banner. That half is printed rather than asserted, because a layer build
//! without submit-time validation is not a bug in this repository — but leaving
//! it unmeasured is worse, since a green local run would then read as "I saw
//! what CI sees" when it did not.

use crate::harness::Headless;
use crcbl_hal::{
    BufferDesc, BufferUsage, CommandEncoderDesc, Instance, MemoryLocation, PresentInfo, SubmitInfo,
};

/// The other half of the validation gate: prove the messenger is **wired**, not
/// merely quiet.
///
/// Every other test asserts a clean report, and every one of them would pass
/// just as happily against a messenger that was never created — which is the
/// failure mode that turns "zero validation errors" into a slogan. So this test
/// commits a deliberate specification violation and asserts the layer
/// *noticed*.
///
/// The violation is chosen to be caught at **record** time and never submitted:
/// a copy whose size exceeds both buffers. An earlier version of this test used
/// an oversized render area instead, which the layer also catches — and which
/// **segfaults lavapipe**, because a spec violation is undefined behaviour and
/// a software rasteriser is under no obligation to survive one. That is the
/// first place radv and lavapipe disagreed, and it is a good argument for
/// provoking validation with something the driver never executes.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_deliberate_violation_is_caught_by_the_layer() {
    let headless = Headless::open();
    let device = &headless.device;

    let small = |label| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: 64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a small buffer")
    };
    let source = small("deliberately too small (src)");
    let destination = small("deliberately too small (dst)");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("deliberately wrong"),
        queue: headless.queue,
    });
    // The `VUID-vkCmdCopyBuffer-size-*` pair: the region must fit in both
    // buffers. Which digits the pair carries is the layer build's business and
    // not this test's — 1.4.357 reports `-00115` and `-00116` here, where an
    // earlier comment named `-00225`/`-00224` — so the assertion below matches
    // on the entry point instead. Recorded, reported, and then thrown away: the
    // command buffer is never submitted, so no driver ever sees it.
    encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
        src: source,
        src_offset: 0,
        dst: destination,
        dst_offset: 0,
        size: 4096,
    });
    let commands = encoder.finish().expect("the backend records it regardless");

    let report = headless.instance.validation_report();
    assert!(
        report.enabled,
        "this suite is meaningless without the validation layer"
    );
    assert!(
        !report.is_clean(),
        "the layer must have caught an out-of-bounds copy; if it did not, every \
         other test in this file is proving nothing"
    );
    assert!(
        report
            .messages
            .iter()
            .any(|message| message.id.contains("CopyBuffer") || message.text.contains("size")),
        "the message should name the copy:\n{}",
        report.summary()
    );

    // Deliberately *not* `headless.finish()`: the report is dirty on purpose.
    device.destroy_command_buffer(commands);
    device.destroy_buffer(source);
    device.destroy_buffer(destination);
    device.wait_idle().expect("idle");
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);
}

/// The same violation again, committed by the **engine** rather than by a test:
/// proof `CRCBL_VK_VALIDATION_PROVOKE` reaches the layer from inside a frame
/// loop.
///
/// `a_deliberate_violation_is_caught_by_the_layer` above is a Rust fixture
/// asserting on a Rust value, which a shell harness running a *binary* has no
/// way to do — and `CRCBL_VK_VALIDATION_SELF_TEST`, the thing those harnesses
/// do have, only ever proves the *report* path is live: its message is
/// submitted through `vkSubmitDebugUtilsMessageEXT` and arrives whatever the
/// layer's checks are set to. So `crcbl-vk` grew a provocation of its own, at
/// the first successful `Device::present`, and this is the test that it fires
/// and that the layer answers it.
///
/// # Why a child process
///
/// The provocation is asked for by an environment variable, which is
/// process-global, and `std::env::set_var` is `unsafe` for exactly that reason.
/// Setting it here would arm the provocation for every *other* test in this
/// binary — each of which ends in `Headless::finish`, which asserts a clean
/// report — so the run with the variable set is a process of its own: this test
/// re-executes the test binary against its own name with the variable in the
/// child's environment. `crcbl_core::trace`'s
/// `the_environment_variable_is_what_turns_the_gate_on` is the same shape and
/// the same argument.
///
/// The child arm is the one that opens a device, and it asserts what the parent
/// cannot see: that a present made the layer speak. The parent asserts the
/// child exited 0 and prints its output when it did not, because a child that
/// failed silently would be this test passing on a provocation that never
/// happened.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_provocation_fires_at_the_first_present_and_the_layer_answers() {
    if std::env::var(crcbl_vk::debug::PROVOKE_VALIDATION_ENV_VAR).is_ok() {
        provoked_child();
        return;
    }

    let output = std::process::Command::new(
        std::env::current_exe().expect("a test binary knows its own path"),
    )
    .args([
        "--exact",
        "validation_gate::the_provocation_fires_at_the_first_present_and_the_layer_answers",
        "--include-ignored",
        "--test-threads",
        "1",
    ])
    .env(crcbl_vk::debug::PROVOKE_VALIDATION_ENV_VAR, "1")
    .env(crcbl_vk::debug::VALIDATION_ENV_VAR, "1")
    .output()
    .expect("the test binary re-executes");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the child run with {}=1 did not pass ({}):\n{stdout}{stderr}",
        crcbl_vk::debug::PROVOKE_VALIDATION_ENV_VAR,
        output.status,
    );
    // **And that it ran the test rather than none.** `--exact` names this
    // function as a string, so renaming the function leaves the child matching
    // nothing — and libtest exits 0 having run nothing, which is this test
    // passing on a provocation that never happened.
    assert!(
        stdout.contains("1 passed"),
        "the child matched no test, so nothing was provoked; the name passed to `--exact` \
         has to be this function's own:\n{stdout}{stderr}"
    );
}

/// The half of the test above that runs with the variable set: one frame to a
/// present, and then the question.
///
/// Deliberately not `Headless::finish`, for the reason its siblings give: the
/// report is dirty on purpose, which is the whole result.
fn provoked_child() {
    let headless = Headless::open();
    let device = &headless.device;

    let frame = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: frame.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");

    let report = headless.instance.validation_report();
    assert!(
        report.enabled,
        "this test is meaningless without the validation layer"
    );
    assert!(
        report
            .messages
            .iter()
            .any(|message| message.id.contains("CopyBuffer")),
        "{}=1 must have recorded an out-of-bounds copy at the first present and the layer \
         must have reported it. A clean report here means either that the provocation never \
         fired or that this layer is loaded and checking nothing — which is exactly the \
         difference the variable exists to expose.\n{}",
        crcbl_vk::debug::PROVOKE_VALIDATION_ENV_VAR,
        report.summary()
    );

    device.wait_idle().expect("idle");
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);
}

/// The **synchronisation** half of the validation gate: prove sync validation is
/// wired, not merely quiet.
///
/// `a_deliberate_violation_is_caught_by_the_layer` does this for ordinary
/// validation. Sync validation is a separate opt-in with a separate failure
/// mode, and `docs/plan/02-vulkan-backend.md` names sync bugs as this stage's
/// headline risk and this layer as the mitigation — so "sync validation is on"
/// has to be a test result rather than an environment variable somebody set.
///
/// It is worth its own test because it was **not** on. Until P1.2,
/// `CRCBL_VK_SYNC_VALIDATION=1` probed for `VK_EXT_validation_features` in the
/// loader's implicit extension list, where it does not appear — it is a *layer*
/// extension — so the flag bought a log line and no checking, here and in CI.
///
/// The hazard is a read-after-write inside one command buffer with no barrier
/// between the two copies. It is recorded and thrown away, never submitted, for
/// the reason the sibling test gives: a spec violation is undefined behaviour,
/// and lavapipe is under no obligation to survive one.
///
/// # Sync validation has two halves, and only one of them is asserted here
///
/// A hazard inside one command buffer is caught while it is being *recorded*.
/// A hazard that spans two command buffers — or two submissions — can only be
/// caught when the queue is submitted, which is a separate piece of the layer
/// (`syncval_submit_time_validation`) and, on some builds, not one that runs.
/// **Every cross-frame hazard is in the second category**, including the
/// write-after-write on the graph's depth transient that this branch fixes: it
/// was reported by the CI leg's layer and by nothing on the developer's, which
/// is how a harness that claims to stand in for CI stops doing so.
///
/// So the second half is *measured* and printed rather than asserted. Asserting
/// it would fail a machine whose layer simply does not implement it, which is
/// not a bug in this repository; leaving it unmeasured is worse, because then
/// "26 tests passed" reads as "I saw what CI sees" when it may not be. The
/// marker line this prints is what `tests/run-vk-e2e.sh` turns into a banner.
///
/// The gate for the *bug class* is deliberately not here at all — it is
/// `crcbl-render`'s `a_second_frame_barriers_against_what_the_first_one_left`,
/// which needs no layer, no ICD and no GPU and therefore cannot be switched off
/// by a distribution's packaging choices.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn synchronisation_validation_catches_a_missing_barrier() {
    let headless = Headless::open();
    let device = &headless.device;
    if !std::env::var("CRCBL_VK_SYNC_VALIDATION").is_ok_and(|value| value == "1") {
        eprintln!("vk e2e: CRCBL_VK_SYNC_VALIDATION is not set; skipping the sync-hazard probe");
        headless.finish();
        return;
    }

    let buffer = |label| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: 4096,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a buffer")
    };
    let (first, second, third) = (buffer("hazard a"), buffer("hazard b"), buffer("hazard c"));

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("deliberate hazard"),
        queue: headless.queue,
    });
    let copy = |src, dst| crcbl_hal::BufferCopy {
        src,
        src_offset: 0,
        dst,
        dst_offset: 0,
        size: 4096,
    };
    // Write `second`, then read it, with nothing ordering the two.
    encoder.copy_buffer_to_buffer(&copy(first, second));
    encoder.copy_buffer_to_buffer(&copy(second, third));
    let commands = encoder.finish().expect("the backend records it regardless");

    let report = headless.instance.validation_report();
    assert!(
        report.enabled,
        "this suite is meaningless without the layer"
    );
    assert!(
        report.messages.iter().any(|message| {
            message.id.contains("SYNC-HAZARD") || message.text.contains("SYNC-HAZARD")
        }),
        "synchronisation validation must have caught a read-after-write with no barrier. If \
         this fails, `CRCBL_VK_SYNC_VALIDATION=1` is buying nothing and the headline risk of \
         this stage is unmitigated.\n{}",
        report.summary()
    );

    // Deliberately not `headless.finish()`: the report is dirty on purpose.
    device.destroy_command_buffer(commands);
    for handle in [first, second, third] {
        device.destroy_buffer(handle);
    }
    device.wait_idle().expect("idle");
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);

    // And the two halves the recording checks cannot see. The marker line is
    // grepped by `tests/run-vk-e2e.sh`; keep the spelling.
    eprintln!(
        "vk e2e: sync-validation reach: record-time=yes one-submission={} cross-submission={}",
        yes_no(queue_hazard_reported(HazardShape::OneSubmission)),
        yes_no(queue_hazard_reported(HazardShape::TwoSubmissions)),
    );
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// How far apart the two halves of a deliberate hazard are placed.
#[derive(Clone, Copy, Debug)]
enum HazardShape {
    /// Two command buffers inside one `vkQueueSubmit2`.
    OneSubmission,
    /// Two separate `vkQueueSubmit2` calls, which is where **every** hazard
    /// that spans a frame boundary lives.
    TwoSubmissions,
}

/// Whether this validation layer reports a write-after-write the *recording*
/// checks cannot see, at the given distance.
///
/// Both shapes are the same hazard — two copies into one buffer with nothing
/// ordering them — and a layer may model one and not the other. The distinction
/// is the whole point of measuring: a frame's barriers are ordered against the
/// *previous frame's submission*, so a layer that reports `OneSubmission` and
/// not `TwoSubmissions` is blind to every cross-frame bug while looking, from
/// a green test run, exactly like one that is not.
///
/// The payload is deliberately large. A layer retires a batch it can prove has
/// completed, and this backend queries the retire timeline after every submit,
/// so a four-kilobyte copy can finish before the next submission is validated
/// and turn a real answer into "no". Thirty-two megabytes will still be in
/// flight; a wrong answer here is then the layer's behaviour rather than the
/// GPU's speed.
///
/// # A `no` here is not a knob somebody forgot to turn on
///
/// Checked on 2026-08-13, against `VK_LAYER_KHRONOS_validation` spec 1.4.357,
/// after a cross-frame write-after-write in the culling-stats ring was found by
/// CI's layer and by nothing here:
///
/// * the layer's own manifest declares `syncval_submit_time_validation` with
///   `default: True`, so submit-time validation is already on;
/// * the layer **does** read `VK_KHRONOS_VALIDATION_*` settings —
///   `VK_KHRONOS_VALIDATION_VALIDATE_SYNC=false` turns record-time sync
///   validation off and makes the test above fail, which is how that was
///   established rather than assumed;
/// * setting `VK_KHRONOS_VALIDATION_SYNCVAL_SUBMIT_TIME_VALIDATION=true`, its
///   `VK_LAYER_`-prefixed spelling, and a `VK_LAYER_SETTINGS_PATH` file saying
///   the same, each change nothing;
/// * and raising `PAYLOAD` to 512 MiB — the theory being that this backend's
///   timeline query after every submit lets the layer retire the first batch
///   before the second is validated — changes nothing either.
///
/// So this answer is a property of the layer build, and CI is the only oracle
/// for the whole hazard class. Anyone tempted to re-run that experiment should
/// read this list first and then go further than it did.
///
/// Its own instance, because the report it produces is dirty by construction and
/// must not land on a caller's.
fn queue_hazard_reported(shape: HazardShape) -> bool {
    /// Big enough to still be running when the next submission is validated.
    const PAYLOAD: u64 = 32 << 20;

    let headless = Headless::open();
    let device = headless.device.as_ref();

    let buffer = |label| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: PAYLOAD,
                usage: BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a buffer")
    };
    let (source, shared, other) = (buffer("reach a"), buffer("reach b"), buffer("reach c"));

    let record = |src, dst| {
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("reach probe"),
            queue: headless.queue,
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src,
            src_offset: 0,
            dst,
            dst_offset: 0,
            size: PAYLOAD,
        });
        encoder.finish().expect("recorded")
    };
    let commands = [record(source, shared), record(other, shared)];

    match shape {
        HazardShape::OneSubmission => {
            device
                .submit(headless.queue, &SubmitInfo::new(&commands))
                .expect("submit");
        }
        HazardShape::TwoSubmissions => {
            for handle in commands {
                device
                    .submit(headless.queue, &SubmitInfo::new(&[handle]))
                    .expect("submit");
            }
        }
    }

    device.wait_idle().expect("idle");
    let report = headless.instance.validation_report();
    let seen = report
        .messages
        .iter()
        .any(|message| message.id.contains("SYNC-HAZARD") || message.text.contains("SYNC-HAZARD"));

    for handle in commands {
        device.destroy_command_buffer(handle);
    }
    for handle in [source, shared, other] {
        device.destroy_buffer(handle);
    }
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);
    seen
}
