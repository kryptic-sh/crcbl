//! A profiler capture of the run's last frames: what the loop records each
//! frame, and the `.crcblprof` file it writes on request.
//!
//! `docs/plan/60-profiler.md`'s P2. Three triggers, one file:
//!
//! - [`CAPTURE_VAR`] names a path; a loop that finds it set turns the CPU trace
//!   on, keeps the last [`DEFAULT_FRAME_WINDOW`] frames of CPU spans and GPU
//!   passes in a [`Recorder`], and writes them to that path as it finishes.
//!   With [`BUDGET_VAR`] set too, it writes them instead as the first frame
//!   over that budget is recorded, CPU or GPU: the frames before the hitch,
//!   ending with it. A GPU frame arrives frames latent, so its capture's CPU
//!   half runs those few frames past it.
//! - The console's `profile_capture <frames> <path>` captures the next
//!   `frames` frames and writes them as the last one is recorded, turning the
//!   CPU trace on for them and off again after if it was off.
//!
//! The file is [`crcbl_profile::Capture`], which any viewer reads without the
//! engine.
//!
//! The GPU half is whatever the run's pass timers measured, frames latent as
//! they always are, and counted only when `CRCBL_PASS_STATISTICS=1` asked the
//! timers to count. Where the device reads its GPU clock, the capture carries
//! a [`ClockPair`] measured as it is written, which puts the passes on the CPU's
//! timeline.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crcbl_core::trace::{RecordKind, Snapshot};
use crcbl_profile::{
    Capture, ClockPair, CpuFrame, Environment, Event, EventKind, GpuFrame, Pass, Statistics, Thread,
};
use crcbl_render::FrameTimings;
pub use crcbl_ui::DEFAULT_FRAME_WINDOW;

/// The variable naming the file a run writes its capture to.
pub const CAPTURE_VAR: &str = "CRCBL_PROFILE_CAPTURE";

/// The variable giving [`CAPTURE_VAR`]'s capture a frame budget, in
/// milliseconds of CPU frame time or of GPU frame time, the timers' elapsed
/// interval: the first frame over it, of either, writes the capture.
pub const BUDGET_VAR: &str = "CRCBL_PROFILE_BUDGET_MS";

/// [`BUDGET_VAR`]'s value as a budget: `None` when unset or empty.
///
/// # Errors
///
/// What a person reads, when the value is not a positive number of
/// milliseconds.
fn budget(value: Option<&str>) -> Result<Option<Duration>, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    value
        .parse::<f64>()
        .ok()
        .filter(|millis| *millis > 0.0)
        .and_then(|millis| Duration::try_from_secs_f64(millis / 1_000.0).ok())
        .map(Some)
        .ok_or_else(|| format!("`{value}` is not a positive number of milliseconds"))
}

/// The most frames one `profile_capture` takes: a minute at 60 Hz. The rings
/// are allocated up front, so a typo of a few zeros must not ask for gigabytes.
pub const MAX_CAPTURE_FRAMES: usize = 3_600;

/// What one `profile_capture` line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureAsk {
    /// How many frames to capture, from the next one on.
    pub frames: usize,
    /// The file the capture is written to.
    pub path: PathBuf,
}

impl CaptureAsk {
    /// `profile_capture`'s arguments: a frame count from 1 to
    /// [`MAX_CAPTURE_FRAMES`], then the file.
    ///
    /// # Errors
    ///
    /// What a person reads, when the line is not that.
    pub fn from_args(args: &[&str]) -> Result<Self, String> {
        let [frames, path] = args else {
            return Err("usage: profile_capture <frames> <path>".to_string());
        };
        let frames = frames
            .parse::<usize>()
            .ok()
            .filter(|frames| (1..=MAX_CAPTURE_FRAMES).contains(frames))
            .ok_or_else(|| {
                format!("`{frames}` is not a frame count from 1 to {MAX_CAPTURE_FRAMES}")
            })?;
        Ok(Self {
            frames,
            path: PathBuf::from(path),
        })
    }
}

/// A capture the loop records, and the file it goes to.
#[derive(Debug)]
pub(crate) struct Requested {
    pub(crate) recorder: Recorder,
    path: PathBuf,
    /// How many CPU frames make the capture complete, for one the console
    /// asked for; `None` for [`CAPTURE_VAR`]'s, written as the run finishes.
    frames: Option<u64>,
    /// Whether the CPU trace was off before this capture turned it on, so
    /// writing it turns the trace off again.
    trace_was_off: bool,
    /// Whether the next CPU frame began before the trace was on: a console
    /// line runs partway through a frame, so that frame's trace holds its
    /// phases without the `frame` span around them, and is not recorded.
    partial_frame: bool,
    /// [`BUDGET_VAR`]'s budget: the first CPU or GPU frame over it completes
    /// the capture.
    budget: Option<Duration>,
    /// What completed the capture early, when a frame went over the budget,
    /// as the capture's environment says it.
    trigger: Option<String>,
    /// The loop's time spent copying frames into the recorder, and how many
    /// CPU frames it was spent over: the capture's own cost, as its
    /// environment says it.
    recording: Duration,
}

impl Requested {
    /// The capture [`CAPTURE_VAR`] asks for, or `None` when it is unset or
    /// empty. Turns the CPU trace on, since a capture without spans would hold
    /// half of what it says.
    pub(crate) fn from_env() -> Option<Self> {
        let path = std::env::var_os(CAPTURE_VAR).filter(|path| !path.is_empty())?;
        let budget = budget(std::env::var(BUDGET_VAR).ok().as_deref()).unwrap_or_else(|why| {
            crate::log::error!(
                "profile: {BUDGET_VAR} {why}; the capture is written as the run finishes"
            );
            None
        });
        crcbl_core::trace::set_enabled(true);
        Some(Self {
            recorder: Recorder::new(DEFAULT_FRAME_WINDOW),
            path: path.into(),
            frames: None,
            trace_was_off: false,
            partial_frame: false,
            budget,
            trigger: None,
            recording: Duration::ZERO,
        })
    }

    /// The capture `ask` describes, of the frames from the next one on. Turns
    /// the CPU trace on for them, as [`Requested::from_env`] does.
    pub(crate) fn from_ask(ask: CaptureAsk) -> Self {
        let trace_was_off = !crcbl_core::trace::is_enabled();
        crcbl_core::trace::set_enabled(true);
        Self {
            recorder: Recorder::new(ask.frames),
            path: ask.path,
            frames: Some(ask.frames as u64),
            trace_was_off,
            partial_frame: trace_was_off,
            budget: None,
            trigger: None,
            recording: Duration::ZERO,
        }
    }

    /// Records one frame's CPU spans, unless it is the partial frame the
    /// trace was turned on in, and notes the frame that went over the budget.
    pub(crate) fn record_cpu(&mut self, snapshot: &Snapshot) {
        if std::mem::take(&mut self.partial_frame) {
            return;
        }
        let number = self.recorder.cpu_frames;
        let began = Instant::now();
        self.recorder.record_cpu(snapshot);
        self.recording += began.elapsed();
        if let Some(budget) = self.budget
            && self.trigger.is_none()
            && let Some(cpu) = crate::perf::frame_cpu_time(snapshot)
            && cpu > budget
        {
            self.trigger = Some(format!(
                "CPU frame {number} took {:.3} ms, over the {:.3} ms budget",
                cpu.as_secs_f64() * 1_000.0,
                budget.as_secs_f64() * 1_000.0
            ));
        }
    }

    /// Records the GPU passes of the newest frame the timers resolved, as
    /// [`Recorder::record_gpu`] does, counts the time it took, and notes the
    /// frame that went over the budget.
    pub(crate) fn record_gpu(&mut self, timings: &FrameTimings) {
        let began = Instant::now();
        self.recorder.record_gpu(timings);
        self.recording += began.elapsed();
        if let Some(budget) = self.budget
            && self.trigger.is_none()
            && let Some(elapsed) = timings.elapsed_nanos.map(Duration::from_nanos)
            && elapsed > budget
        {
            self.trigger = Some(format!(
                "GPU frame {} took {:.3} ms, over the {:.3} ms budget",
                timings.frame,
                elapsed.as_secs_f64() * 1_000.0,
                budget.as_secs_f64() * 1_000.0
            ));
        }
    }

    /// Whether this capture holds every frame it asked for, or a frame went
    /// over its budget, and is written now rather than as the run finishes.
    ///
    /// Counted in CPU frames: the GPU's arrive frames latent, so the GPU half
    /// holds those resolved by then, a few frames behind.
    pub(crate) fn is_complete(&self) -> bool {
        self.trigger.is_some()
            || self
                .frames
                .is_some_and(|frames| self.recorder.cpu_frames >= frames)
    }

    /// Writes the capture to its file, with `clock` as its clock pair, and
    /// says in the log where it went or why it did not: the run's own outcome
    /// does not hang on it. Turns the CPU trace back off if this capture turned
    /// it on.
    pub(crate) fn write(self, adapter: Option<&crcbl_hal::AdapterInfo>, clock: Option<ClockPair>) {
        if self.trace_was_off {
            crcbl_core::trace::set_enabled(false);
        }
        let mut environment = environment(adapter);
        environment
            .entries
            .extend(enabled(crcbl_render::timing::statistics_requested()));
        if let Some(trigger) = &self.trigger {
            environment
                .entries
                .push(("trigger".to_string(), trigger.clone()));
        }
        environment
            .entries
            .push(recording_cost(self.recording, self.recorder.cpu_frames));
        let mut capture = self.recorder.capture(environment);
        capture.clock = clock;
        let written = capture
            .encode()
            .map_err(|error| error.to_string())
            .and_then(|bytes| std::fs::write(&self.path, bytes).map_err(|error| error.to_string()));
        match written {
            Ok(()) => crate::log::info!(
                "profile: wrote {} CPU and {} GPU frames to {}",
                capture.cpu.len(),
                capture.gpu.len(),
                self.path.display()
            ),
            Err(why) => crate::log::error!(
                "profile: the capture {CAPTURE_VAR} asked for was not written to {}: {why}",
                self.path.display()
            ),
        }
    }
}

/// The GPU's clock read between two reads of the trace's, as a capture's clock
/// pair, or `None` where `device` does not read its GPU clock, which the log
/// says: the capture's passes then stay on their own clock.
pub(crate) fn clock_pair(device: &dyn crcbl_hal::Device) -> Option<ClockPair> {
    let before = crcbl_core::trace::clock_nanos();
    let gpu = device.gpu_clock();
    let after = crcbl_core::trace::clock_nanos();
    gpu.map(|gpu_nanos| bracketed(before, gpu_nanos, after))
        .map_err(|error| {
            crate::log::info!("profile: the capture's GPU passes stay on their own clock: {error}");
        })
        .ok()
}

/// The pair a GPU reading taken between the CPU reads `before` and `after`
/// makes: the GPU read happened somewhere in that gap, so the CPU time is its
/// middle and the uncertainty half its width, rounded up.
fn bracketed(before: u64, gpu_nanos: u64, after: u64) -> ClockPair {
    let gap = after.saturating_sub(before);
    ClockPair {
        cpu_nanos: before + gap / 2,
        gpu_nanos,
        uncertainty_nanos: gap.div_ceil(2),
    }
}

/// The last frames of a run's CPU spans and GPU passes.
///
/// Two rings rather than one, for the reason a [`Capture`] keeps two runs of
/// frames: the GPU timers report each frame several frames after the CPU
/// finished it, under their own number, so neither half waits for the other.
#[derive(Debug)]
pub struct Recorder {
    capacity: usize,
    cpu: VecDeque<CpuFrame>,
    gpu: VecDeque<GpuFrame>,
    /// How many CPU frames have been recorded, which numbers the next one.
    cpu_frames: u64,
}

impl Recorder {
    /// A recorder keeping the last `capacity` frames of each kind.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cpu: VecDeque::with_capacity(capacity),
            gpu: VecDeque::with_capacity(capacity),
            cpu_frames: 0,
        }
    }

    /// Records one frame's CPU spans and counters.
    pub fn record_cpu(&mut self, snapshot: &Snapshot) {
        let frame = cpu_frame(self.cpu_frames, snapshot);
        self.cpu_frames += 1;
        push_bounded(&mut self.cpu, frame, self.capacity);
    }

    /// Records the GPU passes of the newest frame the timers resolved, unless
    /// that frame is already recorded: the timers hand the same report back
    /// until a new one resolves.
    pub fn record_gpu(&mut self, timings: &FrameTimings) {
        if timings.is_empty()
            || self
                .gpu
                .back()
                .is_some_and(|frame| frame.number == timings.frame)
        {
            return;
        }
        push_bounded(&mut self.gpu, gpu_frame(timings), self.capacity);
    }

    /// The frames held, as a capture taken on `environment`.
    #[must_use]
    pub fn capture(&self, environment: Environment) -> Capture {
        Capture {
            environment,
            clock: None,
            cpu: self.cpu.iter().cloned().collect(),
            gpu: self.gpu.iter().cloned().collect(),
        }
    }
}

/// Appends `frame`, dropping the oldest to stay within `capacity`.
fn push_bounded<T>(ring: &mut VecDeque<T>, frame: T, capacity: usize) {
    if capacity == 0 {
        return;
    }
    if ring.len() == capacity {
        ring.pop_front();
    }
    ring.push_back(frame);
}

/// A trace snapshot as a capture's CPU frame.
fn cpu_frame(number: u64, snapshot: &Snapshot) -> CpuFrame {
    CpuFrame {
        number,
        threads: snapshot
            .threads
            .iter()
            .map(|thread| Thread {
                track: thread.track.get(),
                name: thread.name.clone(),
                events: thread
                    .records
                    .iter()
                    .map(|record| Event {
                        name: record.name.to_string(),
                        kind: match record.kind {
                            RecordKind::SpanBegin => EventKind::Begin,
                            RecordKind::SpanEnd => EventKind::End,
                            RecordKind::Counter(value) => EventKind::Counter(value),
                        },
                        depth: record.depth,
                        nanos: record.nanos,
                    })
                    .collect(),
                dropped: thread.dropped,
            })
            .collect(),
    }
}

/// The timers' report as a capture's GPU frame.
fn gpu_frame(timings: &FrameTimings) -> GpuFrame {
    GpuFrame {
        number: timings.frame,
        passes: timings
            .passes
            .iter()
            .map(|pass| Pass {
                label: pass.label.clone(),
                begin_nanos: pass.begin_nanos,
                end_nanos: pass.begin_nanos.saturating_add(pass.gpu_nanos),
                statistics: pass.statistics.map(statistics),
            })
            .collect(),
    }
}

/// The seam's counts as the capture's, field for field.
const fn statistics(counted: crcbl_hal::PipelineStatistics) -> Statistics {
    Statistics {
        input_vertices: counted.input_vertices,
        input_primitives: counted.input_primitives,
        vertex_invocations: counted.vertex_invocations,
        clipper_invocations: counted.clipper_invocations,
        clipper_primitives: counted.clipper_primitives,
        fragment_invocations: counted.fragment_invocations,
        compute_invocations: counted.compute_invocations,
        task_invocations: counted.task_invocations,
        mesh_invocations: counted.mesh_invocations,
        mesh_primitives: counted.mesh_primitives,
    }
}

/// The capture's own cost as an environment entry: the loop's time copying
/// frames into the recorder, a frame on average over the CPU frames it
/// recorded.
///
/// Only that copy: the CPU trace the capture turned on costs its spans, and
/// the GPU timers cost their queries, which no number here measures.
fn recording_cost(recording: Duration, frames: u64) -> (String, String) {
    let value = if frames == 0 {
        "no frames recorded".to_string()
    } else {
        #[allow(clippy::cast_precision_loss)]
        let micros = recording.as_secs_f64() * 1e6 / frames as f64;
        format!("{micros:.1} us a frame copying into the recorder, over {frames} frames")
    };
    ("recording cost".to_string(), value)
}

/// What a capture turned on to be taken, as environment entries: the CPU
/// trace, which it always enables, and whether the passes were asked to be
/// counted, which perturbs the GPU timings it holds.
///
/// Statistics asked for are not statistics measured: a device that cannot count
/// times the passes only, and its passes carry no counts.
fn enabled(statistics_requested: bool) -> [(String, String); 2] {
    [
        ("cpu trace".to_string(), "on".to_string()),
        (
            "pass statistics".to_string(),
            if statistics_requested {
                "requested (statistics queries perturb the GPU timings)"
            } else {
                "off"
            }
            .to_string(),
        ),
    ]
}

/// What a capture says it was taken on: the adapter, when the loop has one,
/// and the build.
#[must_use]
pub fn environment(adapter: Option<&crcbl_hal::AdapterInfo>) -> Environment {
    let mut entries = Vec::new();
    if let Some(adapter) = adapter {
        entries.push(("adapter".to_string(), adapter.name.clone()));
        entries.push(("backend".to_string(), format!("{:?}", adapter.backend)));
        entries.push(("driver".to_string(), adapter.driver.clone()));
        entries.push(("vendor".to_string(), format!("{:#06x}", adapter.vendor_id)));
    }
    entries.push(("os".to_string(), std::env::consts::OS.to_string()));
    entries.push(("arch".to_string(), std::env::consts::ARCH.to_string()));
    entries.push((
        "profile".to_string(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .to_string(),
    ));
    entries.push(("crcbl".to_string(), env!("CARGO_PKG_VERSION").to_string()));
    Environment { entries }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::trace::{Record, ThreadTrace, ThreadTrack};
    use crcbl_render::PassTiming;

    fn timings(frame: u64) -> FrameTimings {
        FrameTimings {
            frame,
            elapsed_nanos: None,
            passes: vec![PassTiming {
                label: "shadow".to_string(),
                begin_nanos: 1_000,
                gpu_nanos: 36_000,
                statistics: Some(crcbl_hal::PipelineStatistics {
                    task_invocations: Some(1_344),
                    mesh_invocations: None,
                    ..crcbl_hal::PipelineStatistics::default()
                }),
            }],
        }
    }

    /// **`profile_capture` takes a frame count in range and a file**, and
    /// says what is wrong with anything else.
    #[test]
    fn a_capture_ask_takes_a_frame_count_in_range_and_a_file() {
        assert_eq!(
            CaptureAsk::from_args(&["120", "run.crcblprof"]),
            Ok(CaptureAsk {
                frames: 120,
                path: PathBuf::from("run.crcblprof"),
            })
        );
        let max = MAX_CAPTURE_FRAMES.to_string();
        assert!(
            CaptureAsk::from_args(&[&max, "a"]).is_ok(),
            "the cap is in range"
        );
        let over = (MAX_CAPTURE_FRAMES + 1).to_string();
        for refused in [
            &["0", "a"][..],
            &[&over, "a"],
            &["many", "a"],
            &["120"],
            &["120", "a", "b"],
            &[],
        ] {
            assert!(CaptureAsk::from_args(refused).is_err(), "{refused:?}");
        }
    }

    /// **A console capture is written once it holds its frames, and leaves
    /// the trace as it found it**: off before it, on while it records, off
    /// after its file is written; the file decodes to exactly those frames.
    /// The run-long capture never completes early.
    #[test]
    fn a_console_capture_is_written_after_its_frames_and_restores_the_trace() {
        const FRAMES: usize = 3;
        let path = std::env::temp_dir().join(format!(
            "crcbl-profile-capture-{}.crcblprof",
            std::process::id()
        ));
        crcbl_core::trace::set_enabled(false);
        let mut requested = Requested::from_ask(CaptureAsk {
            frames: FRAMES,
            path: path.clone(),
        });
        assert!(crcbl_core::trace::is_enabled(), "on while it records");
        requested.record_cpu(&Snapshot::default());
        assert!(
            requested
                .recorder
                .capture(Environment::default())
                .cpu
                .is_empty(),
            "the frame the trace was turned on in is not recorded"
        );
        for frame in 0..FRAMES {
            assert!(!requested.is_complete(), "complete after {frame} frames");
            requested.record_cpu(&Snapshot::default());
        }
        assert!(requested.is_complete());

        requested.write(None, None);
        assert!(!crcbl_core::trace::is_enabled(), "off again once written");
        let written = Capture::decode(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(written.cpu.len(), FRAMES);

        let mut run_long = Requested {
            recorder: Recorder::new(1),
            path: PathBuf::new(),
            frames: None,
            trace_was_off: false,
            partial_frame: false,
            budget: None,
            trigger: None,
            recording: Duration::ZERO,
        };
        run_long.recorder.record_cpu(&Snapshot::default());
        run_long.recorder.record_cpu(&Snapshot::default());
        assert!(!run_long.is_complete(), "written as the run finishes");
    }

    /// **A GPU reading bracketed by two CPU reads is placed at their middle**,
    /// uncertain by half the gap, rounded up so an odd gap is not understated.
    #[test]
    fn a_bracketed_reading_is_placed_at_the_middle_of_its_gap() {
        assert_eq!(
            bracketed(1_000, 77, 1_401),
            ClockPair {
                cpu_nanos: 1_200,
                gpu_nanos: 77,
                uncertainty_nanos: 201,
            }
        );
        assert_eq!(bracketed(5, 9, 5).uncertainty_nanos, 0);
    }

    /// **A device that does not read its GPU clock gives no pair**, rather
    /// than one placing the passes somewhere false.
    #[test]
    fn a_device_without_a_gpu_clock_gives_no_pair() {
        let instance = crcbl_hal::null::NullInstance::gpu_driven();
        let adapter = crcbl_hal::Instance::adapters(&instance)
            .into_iter()
            .next()
            .expect("the null instance has an adapter");
        let device = crcbl_hal::Instance::create_device(
            &instance,
            &crcbl_hal::DeviceDesc::for_adapter(adapter.id),
        )
        .expect("a null device opens");
        assert_eq!(clock_pair(device.as_ref()), None);
    }

    /// A snapshot holding one whole frame of `millis` milliseconds.
    fn frame_of(millis: u64) -> Snapshot {
        let span = |kind, nanos| Record {
            name: crate::perf::FRAME_SPAN,
            kind,
            depth: 0,
            nanos,
        };
        Snapshot {
            threads: vec![ThreadTrace {
                track: ThreadTrack::new(0),
                name: Some("main".to_string()),
                records: vec![
                    span(RecordKind::SpanBegin, 0),
                    span(RecordKind::SpanEnd, millis * 1_000_000),
                ],
                dropped: 0,
            }],
            refused_tracks: 0,
        }
    }

    /// **The first frame over the budget completes the run-long capture**,
    /// holding the frames before it and ending with it, and the capture says
    /// which frame it was. A frame at the budget is within it.
    #[test]
    fn a_frame_over_the_budget_completes_the_capture_with_the_frames_before_it() {
        let path = std::env::temp_dir().join(format!(
            "crcbl-profile-budget-{}.crcblprof",
            std::process::id()
        ));
        let mut requested = Requested {
            recorder: Recorder::new(DEFAULT_FRAME_WINDOW),
            path: path.clone(),
            frames: None,
            trace_was_off: false,
            partial_frame: false,
            budget: Some(Duration::from_millis(16)),
            trigger: None,
            recording: Duration::ZERO,
        };
        for millis in [8, 16, 12] {
            requested.record_cpu(&frame_of(millis));
            assert!(!requested.is_complete(), "{millis} ms is within the budget");
        }
        requested.record_cpu(&frame_of(40));
        assert!(requested.is_complete());
        assert_eq!(
            requested.trigger.as_deref(),
            Some("CPU frame 3 took 40.000 ms, over the 16.000 ms budget")
        );
        requested.record_cpu(&frame_of(50));
        assert!(
            requested
                .trigger
                .as_deref()
                .is_some_and(|trigger| trigger.contains("frame 3")),
            "the first frame over is the one named"
        );

        let clock = ClockPair {
            cpu_nanos: 1,
            gpu_nanos: 2,
            uncertainty_nanos: 3,
        };
        requested.write(None, Some(clock));
        let written = Capture::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            written.clock,
            Some(clock),
            "the pair it was given is written"
        );
        std::fs::remove_file(&path).unwrap();
        let numbers: Vec<u64> = written.cpu.iter().map(|frame| frame.number).collect();
        assert_eq!(numbers, [0, 1, 2, 3, 4], "the frames before it are held");
        let trigger = written
            .environment
            .entries
            .iter()
            .find(|(key, _)| key == "trigger")
            .map(|(_, value)| value.as_str());
        assert_eq!(
            trigger,
            Some("CPU frame 3 took 40.000 ms, over the 16.000 ms budget")
        );
        let cost = written
            .environment
            .get("recording cost")
            .unwrap_or_default();
        assert!(cost.ends_with("over 5 frames"), "{cost}");
    }

    /// Timers' report of GPU frame `frame`, one pass of `millis` milliseconds.
    fn gpu_frame_of(frame: u64, millis: u64) -> FrameTimings {
        let nanos = millis * 1_000_000;
        FrameTimings {
            passes: vec![PassTiming {
                label: "forward".to_string(),
                begin_nanos: 0,
                gpu_nanos: nanos,
                statistics: None,
            }],
            frame,
            elapsed_nanos: Some(nanos),
        }
    }

    /// **The first GPU frame over the budget completes the capture too**,
    /// ending its GPU half with that frame, and the capture names it by the
    /// timers' number. A frame at the budget is within it.
    #[test]
    fn a_gpu_frame_over_the_budget_completes_the_capture() {
        let mut requested = Requested {
            recorder: Recorder::new(DEFAULT_FRAME_WINDOW),
            path: PathBuf::new(),
            frames: None,
            trace_was_off: false,
            partial_frame: false,
            budget: Some(Duration::from_millis(16)),
            trigger: None,
            recording: Duration::ZERO,
        };
        for (frame, millis) in [(7, 8), (8, 16)] {
            requested.record_cpu(&frame_of(1));
            requested.record_gpu(&gpu_frame_of(frame, millis));
            assert!(!requested.is_complete(), "{millis} ms is within the budget");
        }
        requested.record_gpu(&gpu_frame_of(9, 40));
        assert!(requested.is_complete());
        assert_eq!(
            requested.trigger.as_deref(),
            Some("GPU frame 9 took 40.000 ms, over the 16.000 ms budget")
        );
        let capture = requested.recorder.capture(Environment::default());
        assert_eq!(
            capture.gpu.last().map(|frame| frame.number),
            Some(9),
            "the GPU half ends with the frame over"
        );
    }

    /// **`CRCBL_PROFILE_BUDGET_MS` takes positive milliseconds**, fractions
    /// too, and says what is wrong with anything else.
    #[test]
    fn a_budget_is_a_positive_number_of_milliseconds() {
        assert_eq!(budget(None), Ok(None));
        assert_eq!(budget(Some("")), Ok(None));
        assert_eq!(budget(Some("16")), Ok(Some(Duration::from_millis(16))));
        assert_eq!(
            budget(Some("16.6")),
            Ok(Some(Duration::from_micros(16_600)))
        );
        for refused in ["0", "-1", "fast", "inf", "NaN"] {
            assert!(budget(Some(refused)).is_err(), "{refused}");
        }
    }

    /// **A capture says what recording it cost**, a frame on average, and
    /// says so rather than dividing by zero when it recorded none.
    #[test]
    fn a_capture_says_what_recording_it_cost_a_frame() {
        assert_eq!(
            recording_cost(Duration::from_micros(300), 120),
            (
                "recording cost".to_string(),
                "2.5 us a frame copying into the recorder, over 120 frames".to_string()
            )
        );
        assert_eq!(recording_cost(Duration::ZERO, 0).1, "no frames recorded");
    }

    /// **A capture says what it turned on**, and marks counted passes as
    /// perturbing the timings beside them.
    #[test]
    fn a_capture_says_what_it_turned_on() {
        let entry = |requested: bool, name: &str| {
            enabled(requested)
                .into_iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
        };
        assert_eq!(entry(false, "cpu trace").as_deref(), Some("on"));
        assert_eq!(entry(false, "pass statistics").as_deref(), Some("off"));
        let counted = entry(true, "pass statistics").expect("named either way");
        assert!(
            counted.starts_with("requested") && counted.contains("perturb"),
            "{counted}"
        );
    }

    /// **The rings keep the newest frames**, each its own number: the CPU's
    /// counted by the recorder, the GPU's the timers' own.
    #[test]
    fn the_rings_keep_the_newest_frames_each_numbered_by_its_own_counter() {
        let mut recorder = Recorder::new(2);
        for frame in 0..3 {
            recorder.record_cpu(&Snapshot::default());
            recorder.record_gpu(&timings(40 + frame));
        }
        let capture = recorder.capture(Environment::default());
        let cpu: Vec<u64> = capture.cpu.iter().map(|frame| frame.number).collect();
        let gpu: Vec<u64> = capture.gpu.iter().map(|frame| frame.number).collect();
        assert_eq!(cpu, [1, 2], "the oldest CPU frame is dropped");
        assert_eq!(gpu, [41, 42], "the oldest GPU frame is dropped");
    }

    /// The timers hand the same report back until a new one resolves; it is
    /// recorded once, and an empty report not at all.
    #[test]
    fn a_report_handed_back_again_is_recorded_once() {
        let mut recorder = Recorder::new(8);
        recorder.record_gpu(&timings(7));
        recorder.record_gpu(&timings(7));
        recorder.record_gpu(&FrameTimings::default());
        assert_eq!(recorder.capture(Environment::default()).gpu.len(), 1);
    }

    /// **Every field reaches the capture**: a span's begin and end and a
    /// counter on a named thread, and a pass's place on the GPU clock and its
    /// counts, `None` kept as `None`.
    #[test]
    fn a_frame_reaches_the_capture_field_for_field() {
        let snapshot = Snapshot {
            threads: vec![ThreadTrace {
                track: ThreadTrack::new(2),
                name: Some("main".to_string()),
                records: vec![
                    Record {
                        name: "frame",
                        kind: RecordKind::SpanBegin,
                        depth: 0,
                        nanos: 10,
                    },
                    Record {
                        name: "draws",
                        kind: RecordKind::Counter(5),
                        depth: 1,
                        nanos: 11,
                    },
                    Record {
                        name: "frame",
                        kind: RecordKind::SpanEnd,
                        depth: 0,
                        nanos: 20,
                    },
                ],
                dropped: 3,
            }],
            refused_tracks: 0,
        };
        let mut recorder = Recorder::new(1);
        recorder.record_cpu(&snapshot);
        recorder.record_gpu(&timings(9));
        let capture = recorder.capture(Environment::default());
        assert_eq!(
            capture.cpu,
            [CpuFrame {
                number: 0,
                threads: vec![Thread {
                    track: 2,
                    name: Some("main".to_string()),
                    events: vec![
                        Event {
                            name: "frame".to_string(),
                            kind: EventKind::Begin,
                            depth: 0,
                            nanos: 10,
                        },
                        Event {
                            name: "draws".to_string(),
                            kind: EventKind::Counter(5),
                            depth: 1,
                            nanos: 11,
                        },
                        Event {
                            name: "frame".to_string(),
                            kind: EventKind::End,
                            depth: 0,
                            nanos: 20,
                        },
                    ],
                    dropped: 3,
                }],
            }]
        );
        assert_eq!(
            capture.gpu,
            [GpuFrame {
                number: 9,
                passes: vec![Pass {
                    label: "shadow".to_string(),
                    begin_nanos: 1_000,
                    end_nanos: 37_000,
                    statistics: Some(Statistics {
                        task_invocations: Some(1_344),
                        ..Statistics::default()
                    }),
                }],
            }]
        );
    }
}
