//! A profiler capture: what a profiling run recorded, frame by frame, and the
//! `.crcblprof` file it is written to.
//!
//! **The format is the seam between producers and viewers**
//! (`docs/plan/60-profiler.md`, decision 2). The engine produces captures; the
//! CLI's report, the debug overlay and the editor's panel read them. So this
//! crate depends on nothing of the engine's and nothing that needs a GPU: a
//! capture recorded on one machine opens anywhere this compiles.
//!
//! # What a capture holds
//!
//! An [`Environment`] saying what it was taken on, then two runs of frames: the
//! CPU's ([`CpuFrame`], span and counter events per thread) and the GPU's
//! ([`GpuFrame`], the render graph's passes with their timestamps and, where
//! the run counted them, their pipeline statistics). The CPU half is
//! `crcbl_core::trace`'s record stream, copied rather than referenced so this
//! crate does not depend on that one.
//!
//! **Two runs, not one, because the two are numbered and timed apart.** The GPU
//! timers number the frames they bracket themselves, and report each one frames
//! after the CPU finished it, so a CPU frame and a GPU frame with the same
//! number need not be the same frame; and CPU events are nanoseconds since the
//! trace epoch where GPU passes are nanoseconds on the GPU's timestamp clock.
//! Nothing here pretends either pair agrees. Putting the two on one timeline
//! takes a calibrated clock pair, which a capture will carry when the producer
//! can measure one.
//!
//! # Reading and writing
//!
//! [`Capture::encode`] writes the bytes and [`Capture::decode`] reads them back;
//! see [`format`](mod@format) for the layout and for what the reader refuses.

pub mod format;

pub use format::{FORMAT_VERSION, FormatError, MAGIC};

/// One profiling run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capture {
    /// What the run was taken on.
    pub environment: Environment,
    /// The CPU's frames, oldest first.
    pub cpu: Vec<CpuFrame>,
    /// The GPU's frames, oldest first.
    pub gpu: Vec<GpuFrame>,
}

/// What a capture was taken on: the adapter, the driver, the backend, the build,
/// and whatever else the producer knows, as named values in the order it wrote
/// them.
///
/// Pairs rather than fields, because which facts a producer can report differs
/// by platform and grows with the profiler, and a viewer shows them all; a
/// fixed struct would need a format change for every new fact.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    /// `(name, value)`, in the order written. A name may repeat; a viewer
    /// shows each.
    pub entries: Vec<(String, String)>,
}

impl Environment {
    /// The first value written under `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == name)
            .map(|(_, value)| value.as_str())
    }
}

/// One frame of the CPU's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CpuFrame {
    /// The frame's number, as the loop counted it.
    pub number: u64,
    /// The events, one entry per thread that recorded any.
    pub threads: Vec<Thread>,
}

/// One frame of the GPU's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuFrame {
    /// The frame's number, as the GPU timers counted it.
    pub number: u64,
    /// The passes, in the order the graph ran them.
    pub passes: Vec<Pass>,
}

/// One thread's events within a frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Thread {
    /// The track number the producer gave the thread, stable for a run.
    pub track: u32,
    /// The thread's name, where it had one.
    pub name: Option<String>,
    /// Its events, oldest first.
    pub events: Vec<Event>,
    /// Events the producer had to drop for want of room. Non-zero means the
    /// thread's record has a hole in it.
    pub dropped: u64,
}

/// One thing a thread recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// The span's or counter's name.
    pub name: String,
    /// What happened.
    pub kind: EventKind,
    /// How many spans were open around it: zero for the outermost, and the same
    /// on a span's begin and its end.
    pub depth: u16,
    /// Nanoseconds since the trace epoch, comparable with every other CPU event
    /// in the capture.
    pub nanos: u64,
}

/// What an [`Event`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// A span opened.
    Begin,
    /// The innermost open span closed.
    End,
    /// A counter sampled, with its value.
    Counter(u64),
}

/// One GPU pass of a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pass {
    /// The pass's label, as the render graph named it.
    pub label: String,
    /// When the pass began, in nanoseconds on the GPU's timestamp clock.
    pub begin_nanos: u64,
    /// When it ended, on the same clock.
    pub end_nanos: u64,
    /// What it counted, where the run counted passes.
    pub statistics: Option<Statistics>,
}

impl Pass {
    /// How long the pass took, or zero for a pair whose end precedes its begin,
    /// which a device with no timestamps writes.
    #[must_use]
    pub const fn nanos(&self) -> u64 {
        self.end_nanos.saturating_sub(self.begin_nanos)
    }
}

/// What the GPU counted over a pass: `crcbl_hal::PipelineStatistics`, field
/// for field.
///
/// **`None` is not zero**, as there: `Some(0)` is a stage the device watched do
/// no work, `None` one it could not speak for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    /// Vertices the input assembler read.
    pub input_vertices: Option<u64>,
    /// Primitives the input assembler assembled.
    pub input_primitives: Option<u64>,
    /// Vertex-stage invocations.
    pub vertex_invocations: Option<u64>,
    /// Primitives that reached the clipper.
    pub clipper_invocations: Option<u64>,
    /// Primitives that left the clipper.
    pub clipper_primitives: Option<u64>,
    /// Fragment-stage invocations.
    pub fragment_invocations: Option<u64>,
    /// Compute-stage invocations.
    pub compute_invocations: Option<u64>,
    /// Task (amplification) stage invocations.
    pub task_invocations: Option<u64>,
    /// Mesh-stage invocations.
    pub mesh_invocations: Option<u64>,
    /// Primitives the mesh stage emitted.
    pub mesh_primitives: Option<u64>,
}

impl Statistics {
    /// The counters in the order the format writes them.
    pub(crate) const fn counters(&self) -> [Option<u64>; 10] {
        [
            self.input_vertices,
            self.input_primitives,
            self.vertex_invocations,
            self.clipper_invocations,
            self.clipper_primitives,
            self.fragment_invocations,
            self.compute_invocations,
            self.task_invocations,
            self.mesh_invocations,
            self.mesh_primitives,
        ]
    }

    /// [`counters`](Self::counters)' inverse.
    pub(crate) const fn from_counters(counters: [Option<u64>; 10]) -> Self {
        let [
            input_vertices,
            input_primitives,
            vertex_invocations,
            clipper_invocations,
            clipper_primitives,
            fragment_invocations,
            compute_invocations,
            task_invocations,
            mesh_invocations,
            mesh_primitives,
        ] = counters;
        Self {
            input_vertices,
            input_primitives,
            vertex_invocations,
            clipper_invocations,
            clipper_primitives,
            fragment_invocations,
            compute_invocations,
            task_invocations,
            mesh_invocations,
            mesh_primitives,
        }
    }
}
