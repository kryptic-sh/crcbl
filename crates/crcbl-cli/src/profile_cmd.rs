//! `crcbl profile report` — read a `.crcblprof` capture, summarise it, and
//! write it as Chrome Trace Event JSON on request.
//!
//! A viewer of the capture format and nothing more (`docs/plan/60-profiler.md`,
//! decision 2): it reads what `crcbl::profile` wrote through
//! [`crcbl_profile::Capture::decode`] and never needs the engine that recorded
//! it.
//!
//! # The summary
//!
//! Per CPU span at the outermost depth, and per GPU pass label, the p50 and p95
//! of its cost per frame over the captured frames, by nearest rank as
//! `crcbl bench` reports. A label a frame draws twice is summed within the
//! frame first, as `crcbl_render::PassStats` sums it, so the row is what that
//! label cost the frame.
//!
//! # The trace
//!
//! `--chrome <OUT>` writes the capture for Perfetto and `chrome://tracing`, the
//! export the profiling decisions of 2026-08-13 chose. The CPU is one process,
//! a thread per track, spans as begin/end events and counters as counter
//! events; the GPU is a second process whose passes are complete events with
//! their statistics as arguments. **The two processes are on different clocks**,
//! and the GPU's is named so: a capture carries no calibrated clock pair yet, so
//! nothing places a GPU pass against a CPU span, and the trace does not pretend
//! to.

use std::path::Path;

use crcbl::core::stats::percentile_of;
use crcbl_profile::{Capture, EventKind, Statistics};

use crate::args::ProfileArgs;
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// The Chrome trace's process for the CPU's tracks.
const CPU_PROCESS: u64 = 1;

/// The Chrome trace's process for the GPU's passes.
const GPU_PROCESS: u64 = 2;

/// What the GPU's process is called in a viewer: it says the clock is not the
/// CPU's.
const GPU_PROCESS_NAME: &str = "GPU (its own clock, not aligned with the CPU)";

/// Nanoseconds in the microsecond a Chrome trace's times are written in.
const NANOS_PER_MICRO: f64 = 1_000.0;

/// Runs `crcbl profile report`.
pub fn run(args: &ProfileArgs) -> Result<Outcome, Failure> {
    let bytes = std::fs::read(&args.file)
        .map_err(|error| Failure::new(format!("cannot read {}: {error}", args.file.display())))?;
    let capture = Capture::decode(&bytes).map_err(|error| {
        Failure::new(format!("{} is not a capture: {error}", args.file.display()))
    })?;
    if let Some(out) = &args.chrome {
        write_chrome(&capture, out)?;
    }
    Ok(summarise(&capture, &args.file, args.chrome.as_deref()))
}

/// The p50 and p95 of one row's per-frame costs.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    name: String,
    p50: u64,
    p95: u64,
    frames: usize,
}

/// Groups per-frame `(name, nanos)` costs into rows, in the order names were
/// first seen, each a percentile pair over the frames it appeared in.
fn rows(frames: &[Vec<(String, u64)>]) -> Vec<Row> {
    let mut samples: Vec<(String, Vec<u64>)> = Vec::new();
    for frame in frames {
        // Summed within the frame first, so a label drawn twice is one sample.
        let mut summed: Vec<(&str, u64)> = Vec::new();
        for (name, nanos) in frame {
            match summed.iter_mut().find(|(seen, _)| seen == name) {
                Some((_, total)) => *total = total.saturating_add(*nanos),
                None => summed.push((name, *nanos)),
            }
        }
        for (name, nanos) in summed {
            match samples.iter_mut().find(|(seen, _)| seen == name) {
                Some((_, values)) => values.push(nanos),
                None => samples.push((name.to_string(), vec![nanos])),
            }
        }
    }
    samples
        .into_iter()
        .map(|(name, mut values)| {
            values.sort_unstable();
            Row {
                name,
                p50: percentile_of(&values, 50).expect("a row has a sample"),
                p95: percentile_of(&values, 95).expect("a row has a sample"),
                frames: values.len(),
            }
        })
        .collect()
}

/// Each CPU frame's outermost spans, as `(name, nanos)`: a begin at depth zero
/// paired with the end that closes it on the same thread.
fn cpu_spans(capture: &Capture) -> Vec<Vec<(String, u64)>> {
    capture
        .cpu
        .iter()
        .map(|frame| {
            let mut spans = Vec::new();
            for thread in &frame.threads {
                let mut open: Option<(&str, u64)> = None;
                for event in thread.events.iter().filter(|event| event.depth == 0) {
                    match event.kind {
                        EventKind::Begin => open = Some((&event.name, event.nanos)),
                        EventKind::End => {
                            if let Some((name, begin)) = open.take()
                                && name == event.name
                            {
                                spans.push((name.to_string(), event.nanos.saturating_sub(begin)));
                            }
                        }
                        EventKind::Counter(_) => {}
                    }
                }
            }
            spans
        })
        .collect()
}

/// Each GPU frame's passes, as `(label, nanos)`.
fn gpu_passes(capture: &Capture) -> Vec<Vec<(String, u64)>> {
    capture
        .gpu
        .iter()
        .map(|frame| {
            frame
                .passes
                .iter()
                .map(|pass| (pass.label.clone(), pass.nanos()))
                .collect()
        })
        .collect()
}

/// The summary a person reads and the object `--json` writes.
fn summarise(capture: &Capture, file: &Path, chrome: Option<&Path>) -> Outcome {
    let cpu = rows(&cpu_spans(capture));
    let gpu = rows(&gpu_passes(capture));
    let threads: usize = capture
        .cpu
        .iter()
        .map(|frame| frame.threads.len())
        .max()
        .unwrap_or(0);
    let dropped: u64 = capture
        .cpu
        .iter()
        .flat_map(|frame| &frame.threads)
        .map(|thread| thread.dropped)
        .sum();

    let mut human = format!("profile: {}", file.display());
    for (name, value) in &capture.environment.entries {
        human.push_str(&format!("\n  {name}: {value}"));
    }
    human.push_str(&format!(
        "\ncpu: {} frames, up to {threads} threads, {dropped} events dropped",
        capture.cpu.len()
    ));
    describe(&mut human, &cpu);
    human.push_str(&format!(
        "\ngpu: {} frames (on the GPU's own clock)",
        capture.gpu.len()
    ));
    describe(&mut human, &gpu);
    if let Some(chrome) = chrome {
        human.push_str(&format!("\nchrome trace: {}", chrome.display()));
    }

    let json_rows = |rows: &[Row]| {
        Json::Array(
            rows.iter()
                .map(|row| {
                    Json::Object(vec![
                        ("name", Json::string(&row.name)),
                        ("frames", Json::Unsigned(row.frames as u64)),
                        ("p50_nanos", Json::Unsigned(row.p50)),
                        ("p95_nanos", Json::Unsigned(row.p95)),
                    ])
                })
                .collect(),
        )
    };
    Outcome {
        human,
        json: vec![
            ("file", Json::string(file.display().to_string())),
            (
                "environment",
                Json::Array(
                    capture
                        .environment
                        .entries
                        .iter()
                        .map(|(name, value)| Json::strings([name.as_str(), value.as_str()]))
                        .collect(),
                ),
            ),
            ("cpu_frames", Json::Unsigned(capture.cpu.len() as u64)),
            ("cpu_events_dropped", Json::Unsigned(dropped)),
            ("cpu_spans", json_rows(&cpu)),
            ("gpu_frames", Json::Unsigned(capture.gpu.len() as u64)),
            ("gpu_passes", json_rows(&gpu)),
            (
                "chrome",
                chrome.map_or(Json::Null, |path| Json::string(path.display().to_string())),
            ),
        ],
    }
}

/// One line per row: its name, how many frames it appeared in, p50 and p95.
#[allow(clippy::cast_precision_loss)]
fn describe(human: &mut String, rows: &[Row]) {
    for row in rows {
        human.push_str(&format!(
            "\n  {:<24} {:>4} frames  {:>9.3} / {:>9.3} ms",
            row.name,
            row.frames,
            row.p50 as f64 / 1.0e6,
            row.p95 as f64 / 1.0e6
        ));
    }
}

/// Writes the capture as Chrome Trace Event JSON.
fn write_chrome(capture: &Capture, out: &Path) -> Result<(), Failure> {
    std::fs::write(out, chrome_trace(capture).to_string())
        .map_err(|error| Failure::new(format!("cannot write {}: {error}", out.display())))
}

/// A Chrome trace time: nanoseconds as the microseconds the format counts in.
#[allow(clippy::cast_precision_loss)]
fn micros(nanos: u64) -> Json {
    Json::Double(nanos as f64 / NANOS_PER_MICRO)
}

/// A metadata event naming a process or a thread.
fn metadata(what: &'static str, pid: u64, tid: u64, name: &str) -> Json {
    Json::Object(vec![
        ("name", Json::string(what)),
        ("ph", Json::string("M")),
        ("pid", Json::Unsigned(pid)),
        ("tid", Json::Unsigned(tid)),
        ("args", Json::Object(vec![("name", Json::string(name))])),
    ])
}

/// The counts a pass carries, as the arguments a viewer shows beside it.
fn statistics_args(statistics: &Statistics) -> Json {
    let counters = [
        ("input_vertices", statistics.input_vertices),
        ("input_primitives", statistics.input_primitives),
        ("vertex_invocations", statistics.vertex_invocations),
        ("clipper_invocations", statistics.clipper_invocations),
        ("clipper_primitives", statistics.clipper_primitives),
        ("fragment_invocations", statistics.fragment_invocations),
        ("compute_invocations", statistics.compute_invocations),
        ("task_invocations", statistics.task_invocations),
        ("mesh_invocations", statistics.mesh_invocations),
        ("mesh_primitives", statistics.mesh_primitives),
    ];
    Json::Object(
        counters
            .into_iter()
            .filter_map(|(name, count)| count.map(|count| (name, Json::Unsigned(count))))
            .collect(),
    )
}

/// The capture as one Chrome Trace Event JSON document.
fn chrome_trace(capture: &Capture) -> Json {
    let mut events = vec![
        metadata("process_name", CPU_PROCESS, 0, "CPU"),
        metadata("process_name", GPU_PROCESS, 0, GPU_PROCESS_NAME),
        metadata("thread_name", GPU_PROCESS, 0, "passes"),
    ];
    let mut named: Vec<u32> = Vec::new();
    for frame in &capture.cpu {
        for thread in &frame.threads {
            let tid = u64::from(thread.track);
            if !named.contains(&thread.track) {
                named.push(thread.track);
                let name = thread
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("thread {}", thread.track));
                events.push(metadata("thread_name", CPU_PROCESS, tid, &name));
            }
            for event in &thread.events {
                let mut fields = vec![
                    ("name", Json::string(&event.name)),
                    ("pid", Json::Unsigned(CPU_PROCESS)),
                    ("tid", Json::Unsigned(tid)),
                    ("ts", micros(event.nanos)),
                ];
                match event.kind {
                    EventKind::Begin => fields.push(("ph", Json::string("B"))),
                    EventKind::End => fields.push(("ph", Json::string("E"))),
                    EventKind::Counter(value) => {
                        fields.push(("ph", Json::string("C")));
                        fields.push(("args", Json::Object(vec![("value", Json::Unsigned(value))])));
                    }
                }
                events.push(Json::Object(fields));
            }
        }
    }
    for frame in &capture.gpu {
        for pass in &frame.passes {
            let mut fields = vec![
                ("name", Json::string(&pass.label)),
                ("ph", Json::string("X")),
                ("pid", Json::Unsigned(GPU_PROCESS)),
                ("tid", Json::Unsigned(0)),
                ("ts", micros(pass.begin_nanos)),
                ("dur", micros(pass.nanos())),
            ];
            if let Some(statistics) = &pass.statistics {
                fields.push(("args", statistics_args(statistics)));
            }
            events.push(Json::Object(fields));
        }
    }
    Json::Object(vec![
        ("traceEvents", Json::Array(events)),
        ("displayTimeUnit", Json::string("ns")),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_profile::{CpuFrame, Environment, Event, GpuFrame, Pass, Thread};

    fn event(name: &str, kind: EventKind, depth: u16, nanos: u64) -> Event {
        Event {
            name: name.to_string(),
            kind,
            depth,
            nanos,
        }
    }

    fn pass(label: &str, begin: u64, nanos: u64) -> Pass {
        Pass {
            label: label.to_string(),
            begin_nanos: begin,
            end_nanos: begin + nanos,
            statistics: None,
        }
    }

    /// Two CPU frames of an outer `frame` span around a nested `tick`, and two
    /// GPU frames drawing `shadow` twice and `forward` once.
    fn capture() -> Capture {
        let cpu_frame = |number: u64, length: u64| CpuFrame {
            number,
            threads: vec![Thread {
                track: 0,
                name: Some("main".to_string()),
                events: vec![
                    event("frame", EventKind::Begin, 0, 1_000),
                    event("tick", EventKind::Begin, 1, 1_100),
                    event("draws", EventKind::Counter(12), 2, 1_150),
                    event("tick", EventKind::End, 1, 1_200),
                    event("frame", EventKind::End, 0, 1_000 + length),
                ],
                dropped: 0,
            }],
        };
        let gpu_frame = |number: u64, forward: u64| GpuFrame {
            number,
            passes: vec![
                pass("shadow", 0, 10_000),
                pass("shadow", 20_000, 30_000),
                Pass {
                    statistics: Some(Statistics {
                        fragment_invocations: Some(689_723),
                        ..Statistics::default()
                    }),
                    ..pass("forward", 60_000, forward)
                },
            ],
        };
        Capture {
            environment: Environment {
                entries: vec![("adapter".to_string(), "test".to_string())],
            },
            cpu: vec![cpu_frame(0, 4_000), cpu_frame(1, 6_000)],
            gpu: vec![gpu_frame(7, 100_000), gpu_frame(8, 300_000)],
        }
    }

    /// **A row is the label's cost per frame**: a pass drawn twice is summed
    /// within its frame, and the percentiles are over frames. Only outermost
    /// CPU spans are rows.
    #[test]
    fn rows_are_per_frame_costs_summed_within_a_frame() {
        let capture = capture();
        assert_eq!(
            rows(&cpu_spans(&capture)),
            [Row {
                name: "frame".to_string(),
                p50: 4_000,
                p95: 6_000,
                frames: 2,
            }]
        );
        assert_eq!(
            rows(&gpu_passes(&capture)),
            [
                Row {
                    name: "shadow".to_string(),
                    p50: 40_000,
                    p95: 40_000,
                    frames: 2,
                },
                Row {
                    name: "forward".to_string(),
                    p50: 100_000,
                    p95: 300_000,
                    frames: 2,
                },
            ]
        );
    }

    /// **The trace holds every event, in the units and processes Chrome's
    /// format names**: begin and end and counter events on the CPU's thread,
    /// complete events with a duration and their counts on the GPU's, and the
    /// GPU's process named for its own clock.
    #[test]
    fn the_chrome_trace_holds_every_event_in_microseconds() {
        let trace = chrome_trace(&capture()).to_string();
        assert!(trace.starts_with("{\"traceEvents\":["), "{trace}");
        assert!(trace.contains(GPU_PROCESS_NAME), "{trace}");
        assert!(
            trace.contains(r#"{"name":"frame","pid":1,"tid":0,"ts":1.0,"ph":"B"}"#),
            "{trace}"
        );
        assert!(
            trace.contains(r#"{"name":"frame","pid":1,"tid":0,"ts":5.0,"ph":"E"}"#),
            "{trace}"
        );
        assert!(
            trace.contains(
                r#"{"name":"draws","pid":1,"tid":0,"ts":1.15,"ph":"C","args":{"value":12}}"#
            ),
            "{trace}"
        );
        assert!(
            trace.contains(
                r#"{"name":"forward","ph":"X","pid":2,"tid":0,"ts":60.0,"dur":100.0,"args":{"fragment_invocations":689723}}"#
            ),
            "{trace}"
        );
        assert!(
            trace.contains(
                r#"{"name":"thread_name","ph":"M","pid":1,"tid":0,"args":{"name":"main"}}"#
            ),
            "{trace}"
        );
        let begins = trace.matches(r#""ph":"B""#).count();
        let ends = trace.matches(r#""ph":"E""#).count();
        assert_eq!((begins, ends), (4, 4), "every span has its pair: {trace}");
        assert_eq!(trace.matches(r#""ph":"X""#).count(), 6, "{trace}");
    }
}
