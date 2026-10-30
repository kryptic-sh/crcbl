//! Creating a batch of independent pipelines at once, so their driver compiles
//! overlap.
//!
//! On a cold driver cache a renderer's build is mostly shader compilation, and
//! each pipeline's compile is single-threaded inside the driver. `crcbl-vk`,
//! `crcbl-dx12` and `crcbl-mtl` all release their device lock for that compile,
//! so pipelines created from several threads compile side by side. [`create`]
//! is how a build does that: the caller describes each pipeline as a job, and
//! the jobs run across a [`crcbl_jobs::Pool`] of the spawn seam's threads.
//!
//! # What does not change
//!
//! * **The results come back in the jobs' order**, whichever thread ran each,
//!   so a caller that hands every success to its rollback before propagating an
//!   error releases the same set it would have serially, and the error it
//!   propagates is the lowest-indexed one rather than the first to finish.
//! * **Labels and descriptions are the jobs'**, so every object is created
//!   exactly as it was. Only the order the device sees the creations in, and so
//!   the handle values it hands out, can differ; nothing above the seam orders
//!   anything by a handle's value.
//! * **`service` is called on the calling thread and nowhere else**, before each
//!   round of jobs — see [`create`].
//!
//! # Which devices take the parallel path
//!
//! `concurrent` decides, per backend and with no wildcard arm, so a new
//! backend fails to compile here until somebody has said which it is. It is
//! unlinked here and below because it does not exist on `wasm32`, where a link
//! to it would not resolve. On `wasm32` there is no decision to make: the HAL's traits are not `Send` or
//! `Sync` there (see [`crcbl_hal::threading`]), the browser's WebGPU command
//! stream is ordered, and the batch is always serial.

#[cfg(not(target_arch = "wasm32"))]
use crcbl_hal::BackendKind;
use crcbl_hal::Device;
#[cfg(not(target_arch = "wasm32"))]
use crcbl_jobs::{Pool, Spawn};

/// The most jobs one round starts between two `service` calls.
///
/// **What keeps the gap bounded on a driver that does not compile in
/// parallel.** A round lasts as long as its slowest compile only where the
/// driver really compiles side by side; one that serialises internally runs a
/// round's compiles one after another, so the gap is then this many pipelines.
/// Four keeps that within about four times the serial build's gap — the forward
/// pass's colour pipeline at 0.31 s (Vulkan) and 0.36 s (D3D12) cold, as
/// [`ForwardRenderer::with_scene_serviced`](crate::forward::ForwardRenderer::with_scene_serviced)
/// records, so under 1.5 s — while a driver that does overlap keeps most of
/// the parallel build's gain: measured cold on radv, a cap of two gave back
/// about two fifths of it and four about a sixth.
#[cfg(not(target_arch = "wasm32"))]
const MAX_JOBS_PER_ROUND: usize = 4;

/// One pipeline's creation, as a batch member.
///
/// `Sync` on native because a pool thread runs it through a shared reference;
/// on `wasm32` the device it captures is not `Sync`, and nothing there runs a
/// job anywhere but the calling thread.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type Job<'a, R> = dyn Fn() -> R + Sync + 'a;

/// One pipeline's creation, as a batch member — see the native definition.
#[cfg(target_arch = "wasm32")]
pub(crate) type Job<'a, R> = dyn Fn() -> R + 'a;

/// Runs every job and returns each one's result in the jobs' order.
///
/// **Serially** where `concurrent` says the device should not be called from
/// several threads at once, where the machine has one core, or where the pool
/// cannot get a thread: then `service` runs before each job, exactly as a
/// build calling them one by one would.
///
/// **In parallel** otherwise, in rounds of at most `MAX_JOBS_PER_ROUND` jobs
/// (fewer where the pool has fewer threads, the caller's included), with
/// `service` before each round. On a driver that compiles side by side a round
/// lasts about as long as its slowest compile, so the gap between two
/// `service` calls stays near one pipeline's; on one that serialises its
/// compiles it is at most a round's worth of them.
///
/// Every job runs whatever the others return, so a failure part way through a
/// batch leaves every success in its slot for the caller to release.
///
/// # Panics
///
/// If a job panics. The panic reaches the caller once the rest of that job's
/// round has run — on the serial path every job is a round of its own — and no
/// later job is started. Pipelines the batch had already made are not
/// released.
pub(crate) fn create<R, const N: usize>(
    device: &dyn Device,
    service: &mut dyn FnMut(),
    jobs: [&Job<'_, R>; N],
) -> [R; N]
where
    R: crcbl_hal::threading::HalThreadSafe,
{
    #[cfg(not(target_arch = "wasm32"))]
    if concurrent(device)
        && let Some(pool) = pool_for(crcbl_jobs::default_spawner().as_ref(), N)
    {
        return in_parallel(pool, service, jobs);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = device;
    in_order(service, jobs)
}

/// Whether `device`'s backend should have pipelines created on it from
/// several threads at once.
///
/// Every native [`Device`] is `Send + Sync`, so the parallel path is always
/// *safe*; this is the question of whether it is *wanted*:
///
/// * **Vulkan, D3D12 and Metal: yes.** Each releases its device lock for the
///   driver's compile, which is the whole gain.
/// * **WebGPU: no.** Its calls are encoded into one ordered command stream, and
///   the browser that replays it has one thread.
/// * **Null: no.** Its creations cost nothing, so there is nothing to overlap,
///   and its recorder's event order is what tests read — which would then
///   depend on the scheduler.
#[cfg(not(target_arch = "wasm32"))]
fn concurrent(device: &dyn Device) -> bool {
    match device.backend() {
        BackendKind::Vulkan | BackendKind::Dx12 | BackendKind::Metal => true,
        BackendKind::WebGpu | BackendKind::Null => false,
    }
}

/// A pool on `spawner` sized for a batch of `jobs`, or `None` where one would
/// not help: one job, one core, or a runtime that will not start a thread.
#[cfg(not(target_arch = "wasm32"))]
fn pool_for(spawner: &dyn Spawn, jobs: usize) -> Option<Pool> {
    // The calling thread runs jobs too, so a pool of `n` workers runs `n + 1`,
    // and no round runs more than `MAX_JOBS_PER_ROUND` of them.
    let workers = jobs
        .min(MAX_JOBS_PER_ROUND)
        .min(spawner.parallelism().get())
        .saturating_sub(1);
    if workers == 0 {
        return None;
    }
    // A refused thread costs the batch its parallelism and nothing else: the
    // jobs are all still here, and they run serially instead.
    Pool::with_workers(spawner, workers)
        .ok()
        .filter(|pool| pool.workers() > 0)
}

/// [`create`]'s serial path.
fn in_order<R, const N: usize>(service: &mut dyn FnMut(), jobs: [&Job<'_, R>; N]) -> [R; N] {
    jobs.map(|job| {
        service();
        job()
    })
}

/// [`create`]'s parallel path: rounds of `pool.workers() + 1` jobs, capped at
/// `MAX_JOBS_PER_ROUND`, with `service` before each.
#[cfg(not(target_arch = "wasm32"))]
fn in_parallel<R, const N: usize>(
    mut pool: Pool,
    service: &mut dyn FnMut(),
    jobs: [&Job<'_, R>; N],
) -> [R; N]
where
    R: Send,
{
    let round = (pool.workers() + 1).min(MAX_JOBS_PER_ROUND);
    let mut slots: [Option<R>; N] = core::array::from_fn(|_| None);
    for (round_jobs, round_slots) in jobs.chunks(round).zip(slots.chunks_mut(round)) {
        service();
        // A chunk of one: each job is its own unit of work, so no thread waits
        // on a compile another could have taken.
        pool.par_for(round_slots, 1, |index, slot| {
            slot[0] = Some(round_jobs[index]());
        });
    }
    slots.map(|slot| slot.unwrap_or_else(|| unreachable!("par_for runs every chunk once")))
}

// Native only: the parallel path does not exist on `wasm32`, and the null
// device is opened through `Instance::create_device`, which is native only too.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use core::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::draw_gen::compute_pipeline;
    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{
        AdapterId, ComputePipelineHandle, DeviceDesc, HalError, Instance, PipelineLayoutDesc,
        PipelineLayoutHandle,
    };
    use crcbl_jobs::SpawnError;
    use crcbl_shaders::{CLEAR_COUNTERS, clear_counters};

    /// How long a job waits for another to be running beside it before the
    /// test calls the batch serial. Generous: a worker that has to be woken
    /// from its condition variable is microseconds late, not seconds.
    const OVERLAP_PATIENCE: Duration = Duration::from_secs(10);

    fn open() -> (Recorder, Box<dyn Device>) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let device = instance
            .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
            .expect("the null device opens");
        (recorder, device)
    }

    fn layout(device: &dyn Device, label: &str) -> PipelineLayoutHandle {
        device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some(label),
                bind_group_layouts: &[],
                push_constants: None,
            })
            .expect("an empty layout is created")
    }

    /// A pipeline labelled `label` on `layout`, paired with the label so a
    /// slot can say which job filled it.
    fn labelled(
        device: &dyn Device,
        label: &'static str,
        layout: PipelineLayoutHandle,
    ) -> Result<(&'static str, ComputePipelineHandle), HalError> {
        compute_pipeline(
            device,
            label,
            &CLEAR_COUNTERS,
            layout,
            clear_counters::WORKGROUP_SIZE,
        )
        .map(|pipeline| (label, pipeline))
    }

    /// **The parallel path really runs two jobs at once, hands every result
    /// back in its job's slot, and keeps the successes of a batch whose middle
    /// member was refused** — so a caller that releases every success before
    /// propagating the error leaves nothing behind.
    ///
    /// Driven through [`in_parallel`] because the null device never takes that
    /// path by itself (see [`concurrent`]). The refusal is a pipeline naming a
    /// layout already destroyed, which the null device checks. Overlap is
    /// observed rather than assumed: the first job does not finish until
    /// another has started, which only a second thread can do while the first
    /// is still running.
    #[test]
    fn the_parallel_path_overlaps_and_keeps_every_success_in_its_slot() {
        let (recorder, device) = open();
        let device = device.as_ref();
        let layout = layout(device, "batch");
        let gone = self::layout(device, "gone");
        device.destroy_pipeline_layout(gone);
        let before = recorder.total_live_objects();

        // Each side waits for the other with a deadline, so a serial run fails
        // the assertion below instead of hanging — whichever order it took.
        let first_running = AtomicBool::new(false);
        let other_beside = AtomicBool::new(false);
        let wait_for = |flag: &AtomicBool| {
            let deadline = Instant::now() + OVERLAP_PATIENCE;
            while !flag.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::yield_now();
            }
            flag.load(Ordering::Acquire)
        };
        let first = || {
            first_running.store(true, Ordering::Release);
            let overlapped = wait_for(&other_beside);
            first_running.store(false, Ordering::Release);
            labelled(device, "a", layout).map(|made| (made, overlapped))
        };
        let other = |label: &'static str, layout| {
            let (first_running, other_beside) = (&first_running, &other_beside);
            move || {
                if label == "b" && wait_for(first_running) {
                    other_beside.store(true, Ordering::Release);
                }
                labelled(device, label, layout).map(|made| (made, true))
            }
        };
        let (b, refused, c, d) = (
            other("b", layout),
            other("refused", gone),
            other("c", layout),
            other("d", layout),
        );
        let jobs: [&Job<'_, _>; 5] = [&first, &b, &refused, &c, &d];
        let pool = Pool::with_workers(
            crcbl_jobs::default_spawner().as_ref(),
            MAX_JOBS_PER_ROUND - 1,
        )
        .expect("the test's pool starts");
        assert!(pool.workers() > 0, "this test needs a threaded runtime");
        let mut services = 0;
        let results = in_parallel(pool, &mut || services += 1, jobs);

        assert_eq!(
            services,
            jobs.len().div_ceil(MAX_JOBS_PER_ROUND),
            "`service` before each round"
        );
        assert!(
            results[0].as_ref().is_ok_and(|(_, overlapped)| *overlapped),
            "the first job finished without another having started beside it, \
             so the batch ran serially: {results:?}"
        );
        assert!(
            matches!(results[2], Err(HalError::InvalidHandle { .. })),
            "the member naming a destroyed layout is refused: {results:?}"
        );
        let made: Vec<(&str, ComputePipelineHandle)> =
            results.iter().flatten().map(|(made, _)| *made).collect();
        assert_eq!(
            made.iter().map(|(label, _)| *label).collect::<Vec<_>>(),
            ["a", "b", "c", "d"],
            "every other member made its pipeline and is in its own slot"
        );

        for (_, pipeline) in made {
            device.destroy_compute_pipeline(pipeline);
        }
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "releasing what the batch handed back released everything it made"
        );
        device.destroy_pipeline_layout(layout);
        recorder.assert_valid();
    }

    /// **A batch longer than a round runs as several, `service` before each,
    /// and every job's result still lands in its own slot** — the round's
    /// local index is not the batch's.
    ///
    /// A pool of one worker makes a round of two, so five jobs are three
    /// rounds whatever `MAX_JOBS_PER_ROUND` is.
    #[test]
    fn a_batch_of_several_rounds_keeps_every_result_in_its_slot() {
        let (recorder, device) = open();
        let device = device.as_ref();
        let layout = layout(device, "batch");
        let job = |label: &'static str| move || labelled(device, label, layout);
        let (a, b, c, d, e) = (job("a"), job("b"), job("c"), job("d"), job("e"));
        let pool = Pool::with_workers(crcbl_jobs::default_spawner().as_ref(), 1)
            .expect("the test's pool starts");
        assert_eq!(pool.workers(), 1, "this test needs a threaded runtime");
        let mut services = 0;
        let results = in_parallel(pool, &mut || services += 1, [&a, &b, &c, &d, &e]);

        assert_eq!(
            services, 3,
            "rounds of two, three of them, `service` before each"
        );
        let labels: Vec<&str> = results
            .iter()
            .map(|result| result.as_ref().expect("each member is created").0)
            .collect();
        assert_eq!(
            labels,
            ["a", "b", "c", "d", "e"],
            "each result in its job's slot"
        );
        for result in results {
            device.destroy_compute_pipeline(result.expect("checked above").1);
        }
        device.destroy_pipeline_layout(layout);
        recorder.assert_valid();
    }

    /// A runtime that promises threads and then refuses one.
    #[derive(Debug)]
    struct Refuses;

    /// A runtime with cores and no way to start a thread on them — the
    /// browser's before its page has announced a worker shim. Unlike
    /// [`crcbl_jobs::Inline`] it reports more than one, so `pool_for` gets as
    /// far as building a pool, which then has no workers.
    #[derive(Debug)]
    struct Unthreaded;

    impl Spawn for Unthreaded {
        fn threaded(&self) -> bool {
            false
        }

        fn parallelism(&self) -> core::num::NonZeroUsize {
            core::num::NonZeroUsize::new(8).expect("eight is not zero")
        }

        fn spawn(&self, name: &'static str, work: crcbl_jobs::Work) -> Result<(), SpawnError> {
            drop(work);
            Err(SpawnError::NoThreads(name))
        }
    }

    impl Spawn for Refuses {
        fn threaded(&self) -> bool {
            true
        }

        fn parallelism(&self) -> core::num::NonZeroUsize {
            core::num::NonZeroUsize::new(8).expect("eight is not zero")
        }

        fn spawn(&self, name: &'static str, work: crcbl_jobs::Work) -> Result<(), SpawnError> {
            drop(work);
            Err(SpawnError::NoThreads(name))
        }
    }

    /// **No pool means the serial path, never a failed build**: a runtime with
    /// no threads, one that refuses the thread it promised, and a batch of one
    /// all get `None` from `pool_for` — and a batch that can use one gets a pool
    /// no wider than a round.
    #[test]
    fn a_batch_without_threads_falls_back_to_serial() {
        assert!(
            pool_for(&crcbl_jobs::Inline, 5).is_none(),
            "a runtime with no threads"
        );
        assert!(
            pool_for(&Unthreaded, 5).is_none(),
            "a runtime with cores and no threads"
        );
        assert!(pool_for(&Refuses, 5).is_none(), "a refused thread");
        let spawner = crcbl_jobs::default_spawner();
        assert!(pool_for(spawner.as_ref(), 1).is_none(), "a batch of one");
        if spawner.parallelism().get() >= MAX_JOBS_PER_ROUND {
            let pool = pool_for(spawner.as_ref(), 5).expect("a threaded runtime gets a pool");
            assert_eq!(
                pool.workers() + 1,
                MAX_JOBS_PER_ROUND,
                "the pool is as wide as a round and no wider"
            );
        }
    }

    /// **The null device takes the serial path**, with `service` before every
    /// member — see [`concurrent`] for why it is not a parallel backend — and
    /// the results come back in the jobs' order there too.
    #[test]
    fn the_null_device_creates_a_batch_in_order_on_the_calling_thread() {
        let (recorder, device) = open();
        let device = device.as_ref();
        let layout = layout(device, "batch");
        let caller = std::thread::current().id();
        let job = |label: &'static str| {
            move || {
                let here = std::thread::current().id() == caller;
                labelled(device, label, layout).map(|made| (made, here))
            }
        };
        let (a, b, c) = (job("a"), job("b"), job("c"));
        let mut services = 0;
        let results = create(device, &mut || services += 1, [&a, &b, &c]);

        assert_eq!(services, 3, "`service` before each of the three");
        let made: Vec<(&str, bool)> = results
            .iter()
            .map(|result| {
                let ((label, _), here) = result.as_ref().expect("each member is created");
                (*label, *here)
            })
            .collect();
        assert_eq!(made, [("a", true), ("b", true), ("c", true)]);
        let created: Vec<Option<String>> = recorder
            .pipelines_created()
            .into_iter()
            .map(|record| record.label)
            .collect();
        assert_eq!(
            created,
            [Some("a".into()), Some("b".into()), Some("c".into())],
            "the device saw them in the jobs' order"
        );
        for result in results {
            let ((_, pipeline), _) = result.expect("checked above");
            device.destroy_compute_pipeline(pipeline);
        }
        device.destroy_pipeline_layout(layout);
        recorder.assert_valid();
    }
}
