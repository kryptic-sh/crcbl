//! The `.crcblprof` file: layout, writer and reader.
//!
//! # Layout (format version 1)
//!
//! Little-endian throughout, as every format this workspace writes.
//!
//! ```text
//! magic:        b"CRCBLPRF"
//! version:      u16
//! environment:  u32 count, then count × (string name, string value)
//! cpu:          u32 count, then count × CpuFrame
//! gpu:          u32 count, then count × GpuFrame
//!
//! CpuFrame: u64 number, u32 thread count, Thread × count
//! GpuFrame: u64 number, u32 pass count, Pass × count
//! Thread:  u32 track, optional string name, u64 dropped, u32 event count, Event × count
//! Event:   string name, u8 kind (0 begin, 1 end, 2 counter then u64 value),
//!          u16 depth, u64 nanos
//! Pass:    string label, u64 begin, u64 end, optional Statistics
//! Statistics: u16 mask, bit i set when counter i is present, then one u64 per
//!          set bit in counter order (input vertices, input primitives, vertex,
//!          clipper, clipper out, fragment, compute, task, mesh, mesh primitives)
//!
//! string:   u32 byte length, then UTF-8
//! optional: u8 0 for none, 1 then the value
//! ```
//!
//! # What the reader refuses
//!
//! Anything that is not exactly a capture this build can read: another file's
//! magic, a later version, a file ending inside a field, a tag or mask bit the
//! layout does not define, text that is not UTF-8, and bytes after the last
//! GPU frame. A count is never trusted with an allocation: elements are read one
//! at a time, so a count larger than the file runs out of bytes and says so
//! rather than reserving memory for it.

use crate::{Capture, CpuFrame, Environment, Event, EventKind, GpuFrame, Pass, Statistics, Thread};

/// The first eight bytes of every capture.
pub const MAGIC: &[u8; 8] = b"CRCBLPRF";

/// The layout version this build writes, and the newest it reads.
pub const FORMAT_VERSION: u16 = 1;

/// The statistics counters a capture can carry, which is how many mask bits
/// are defined.
const COUNTERS: usize = 10;

/// Why a capture could not be written or read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FormatError {
    /// The bytes do not start with [`MAGIC`].
    #[error("not a profiler capture: the file does not start with CRCBLPRF")]
    BadMagic,
    /// Written at a version this build does not know.
    #[error("capture format version {found} is newer than this build reads ({current})")]
    Newer {
        /// The version the file says it is.
        found: u16,
        /// [`FORMAT_VERSION`].
        current: u16,
    },
    /// The file ends inside a field.
    #[error("capture truncated in {0}")]
    Truncated(&'static str),
    /// A tag or mask bit the layout does not define.
    #[error("capture has an undefined {what} tag {tag:#x}")]
    BadTag {
        /// The field it was read for.
        what: &'static str,
        /// The value found.
        tag: u16,
    },
    /// Text that is not UTF-8.
    #[error("capture text in {0} is not UTF-8")]
    NotUtf8(&'static str),
    /// Bytes after the last GPU frame.
    #[error("capture has {0} bytes after its last frame")]
    TrailingBytes(usize),
    /// A count or a string too long for its `u32` length field.
    #[error("{0} is too long for a capture's u32 length field")]
    TooLong(&'static str),
}

impl Capture {
    /// The capture as `.crcblprof` bytes.
    ///
    /// # Errors
    ///
    /// [`FormatError::TooLong`] for a count or a string past `u32::MAX`, which
    /// the layout's length fields cannot say.
    pub fn encode(&self) -> Result<Vec<u8>, FormatError> {
        let mut out = Writer(Vec::new());
        out.0.extend_from_slice(MAGIC);
        out.u16(FORMAT_VERSION);
        out.count(self.environment.entries.len(), "the environment")?;
        for (name, value) in &self.environment.entries {
            out.text(name, "an environment name")?;
            out.text(value, "an environment value")?;
        }
        out.count(self.cpu.len(), "the CPU frames")?;
        for frame in &self.cpu {
            out.cpu_frame(frame)?;
        }
        out.count(self.gpu.len(), "the GPU frames")?;
        for frame in &self.gpu {
            out.gpu_frame(frame)?;
        }
        Ok(out.0)
    }

    /// Reads a capture from `.crcblprof` bytes.
    ///
    /// # Errors
    ///
    /// See the [module docs](self) for each refusal.
    pub fn decode(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut reader = Reader { bytes };
        if reader.take(MAGIC.len(), "the magic")? != MAGIC {
            return Err(FormatError::BadMagic);
        }
        let version = reader.u16("the version")?;
        if version > FORMAT_VERSION {
            return Err(FormatError::Newer {
                found: version,
                current: FORMAT_VERSION,
            });
        }
        let mut environment = Environment::default();
        for _ in 0..reader.u32("the environment count")? {
            let name = reader.text("an environment name")?;
            let value = reader.text("an environment value")?;
            environment.entries.push((name, value));
        }
        let mut cpu = Vec::new();
        for _ in 0..reader.u32("the CPU frame count")? {
            cpu.push(reader.cpu_frame()?);
        }
        let mut gpu = Vec::new();
        for _ in 0..reader.u32("the GPU frame count")? {
            gpu.push(reader.gpu_frame()?);
        }
        if !reader.bytes.is_empty() {
            return Err(FormatError::TrailingBytes(reader.bytes.len()));
        }
        Ok(Self {
            environment,
            cpu,
            gpu,
        })
    }
}

/// The bytes being written.
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn count(&mut self, count: usize, what: &'static str) -> Result<(), FormatError> {
        self.u32(u32::try_from(count).map_err(|_| FormatError::TooLong(what))?);
        Ok(())
    }

    fn text(&mut self, text: &str, what: &'static str) -> Result<(), FormatError> {
        self.count(text.len(), what)?;
        self.0.extend_from_slice(text.as_bytes());
        Ok(())
    }

    fn cpu_frame(&mut self, frame: &CpuFrame) -> Result<(), FormatError> {
        self.u64(frame.number);
        self.count(frame.threads.len(), "a frame's threads")?;
        for thread in &frame.threads {
            self.u32(thread.track);
            match &thread.name {
                None => self.u8(0),
                Some(name) => {
                    self.u8(1);
                    self.text(name, "a thread name")?;
                }
            }
            self.u64(thread.dropped);
            self.count(thread.events.len(), "a thread's events")?;
            for event in &thread.events {
                self.text(&event.name, "an event name")?;
                match event.kind {
                    EventKind::Begin => self.u8(0),
                    EventKind::End => self.u8(1),
                    EventKind::Counter(value) => {
                        self.u8(2);
                        self.u64(value);
                    }
                }
                self.u16(event.depth);
                self.u64(event.nanos);
            }
        }
        Ok(())
    }

    fn gpu_frame(&mut self, frame: &GpuFrame) -> Result<(), FormatError> {
        self.u64(frame.number);
        self.count(frame.passes.len(), "a frame's passes")?;
        for pass in &frame.passes {
            self.text(&pass.label, "a pass label")?;
            self.u64(pass.begin_nanos);
            self.u64(pass.end_nanos);
            match &pass.statistics {
                None => self.u8(0),
                Some(statistics) => {
                    self.u8(1);
                    let counters = statistics.counters();
                    let mask = counters
                        .iter()
                        .enumerate()
                        .filter(|(_, counter)| counter.is_some())
                        .fold(0_u16, |mask, (bit, _)| mask | 1 << bit);
                    self.u16(mask);
                    for value in counters.into_iter().flatten() {
                        self.u64(value);
                    }
                }
            }
        }
        Ok(())
    }
}

/// The unread rest of a capture.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize, what: &'static str) -> Result<&'a [u8], FormatError> {
        if self.bytes.len() < len {
            return Err(FormatError::Truncated(what));
        }
        let (taken, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N], FormatError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N, what)?);
        Ok(out)
    }

    fn u8(&mut self, what: &'static str) -> Result<u8, FormatError> {
        self.array::<1>(what).map(|[byte]| byte)
    }

    fn u16(&mut self, what: &'static str) -> Result<u16, FormatError> {
        self.array(what).map(u16::from_le_bytes)
    }

    fn u32(&mut self, what: &'static str) -> Result<u32, FormatError> {
        self.array(what).map(u32::from_le_bytes)
    }

    fn u64(&mut self, what: &'static str) -> Result<u64, FormatError> {
        self.array(what).map(u64::from_le_bytes)
    }

    fn text(&mut self, what: &'static str) -> Result<String, FormatError> {
        let len = self.u32(what)?;
        // A `u32` fits a `usize` on every target this builds for, and `take`
        // checks it against the bytes left before anything is copied.
        let bytes = self.take(len as usize, what)?;
        let text = core::str::from_utf8(bytes).map_err(|_| FormatError::NotUtf8(what))?;
        Ok(text.to_owned())
    }

    /// The optional field's tag: whether a value follows.
    fn present(&mut self, what: &'static str) -> Result<bool, FormatError> {
        match self.u8(what)? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(FormatError::BadTag {
                what,
                tag: u16::from(tag),
            }),
        }
    }

    fn cpu_frame(&mut self) -> Result<CpuFrame, FormatError> {
        let number = self.u64("a CPU frame number")?;
        let mut threads = Vec::new();
        for _ in 0..self.u32("a frame's thread count")? {
            let track = self.u32("a thread track")?;
            let name = if self.present("a thread name")? {
                Some(self.text("a thread name")?)
            } else {
                None
            };
            let dropped = self.u64("a thread's dropped count")?;
            let mut events = Vec::new();
            for _ in 0..self.u32("a thread's event count")? {
                let name = self.text("an event name")?;
                let kind = match self.u8("an event kind")? {
                    0 => EventKind::Begin,
                    1 => EventKind::End,
                    2 => EventKind::Counter(self.u64("a counter value")?),
                    tag => {
                        return Err(FormatError::BadTag {
                            what: "an event kind",
                            tag: u16::from(tag),
                        });
                    }
                };
                let depth = self.u16("an event depth")?;
                let nanos = self.u64("an event time")?;
                events.push(Event {
                    name,
                    kind,
                    depth,
                    nanos,
                });
            }
            threads.push(Thread {
                track,
                name,
                events,
                dropped,
            });
        }
        Ok(CpuFrame { number, threads })
    }

    fn gpu_frame(&mut self) -> Result<GpuFrame, FormatError> {
        let number = self.u64("a GPU frame number")?;
        let mut passes = Vec::new();
        for _ in 0..self.u32("a frame's pass count")? {
            let label = self.text("a pass label")?;
            let begin_nanos = self.u64("a pass begin")?;
            let end_nanos = self.u64("a pass end")?;
            let statistics = if self.present("a pass's statistics")? {
                let mask = self.u16("a statistics mask")?;
                if mask >> COUNTERS != 0 {
                    return Err(FormatError::BadTag {
                        what: "a statistics mask",
                        tag: mask,
                    });
                }
                let mut counters = [None; COUNTERS];
                for (bit, counter) in counters.iter_mut().enumerate() {
                    if mask & (1 << bit) != 0 {
                        *counter = Some(self.u64("a statistics counter")?);
                    }
                }
                Some(Statistics::from_counters(counters))
            } else {
                None
            };
            passes.push(Pass {
                label,
                begin_nanos,
                end_nanos,
                statistics,
            });
        }
        Ok(GpuFrame { number, passes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A capture that uses every part of the layout: two environment entries,
    /// a named and an unnamed thread, all three event kinds, a pass with
    /// statistics (some counters absent) and one without, and a second frame of
    /// each kind.
    fn every_field() -> Capture {
        Capture {
            environment: Environment {
                entries: vec![
                    ("adapter".to_string(), "AMD Radeon RX 7900 XTX".to_string()),
                    ("backend".to_string(), "vulkan".to_string()),
                ],
            },
            cpu: vec![
                CpuFrame {
                    number: 41,
                    threads: vec![
                        Thread {
                            track: 0,
                            name: Some("main".to_string()),
                            events: vec![
                                Event {
                                    name: "frame".to_string(),
                                    kind: EventKind::Begin,
                                    depth: 0,
                                    nanos: 1_000,
                                },
                                Event {
                                    name: "draws".to_string(),
                                    kind: EventKind::Counter(128),
                                    depth: 1,
                                    nanos: 1_500,
                                },
                                Event {
                                    name: "frame".to_string(),
                                    kind: EventKind::End,
                                    depth: 0,
                                    nanos: 9_000,
                                },
                            ],
                            dropped: 0,
                        },
                        Thread {
                            track: 3,
                            name: None,
                            events: Vec::new(),
                            dropped: 7,
                        },
                    ],
                },
                CpuFrame {
                    number: 42,
                    threads: Vec::new(),
                },
            ],
            gpu: vec![
                GpuFrame {
                    number: 38,
                    passes: vec![
                        Pass {
                            label: "shadow".to_string(),
                            begin_nanos: 500_000,
                            end_nanos: 536_000,
                            statistics: Some(Statistics {
                                clipper_invocations: Some(746),
                                fragment_invocations: Some(0),
                                task_invocations: Some(1_344),
                                mesh_invocations: Some(3_136),
                                mesh_primitives: Some(746),
                                ..Statistics::default()
                            }),
                        },
                        Pass {
                            label: "tonemap".to_string(),
                            begin_nanos: 600_000,
                            end_nanos: 608_000,
                            statistics: None,
                        },
                    ],
                },
                GpuFrame {
                    number: 39,
                    passes: Vec::new(),
                },
            ],
        }
    }

    /// The committed version-1 capture: the bytes [`every_field`] encodes to.
    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/every-field-v1.crcblprof");

    /// **What is written reads back as what was written**, every field of it.
    #[test]
    fn a_capture_reads_back_as_it_was_written() {
        let capture = every_field();
        let bytes = capture.encode().expect("encodes");
        assert_eq!(Capture::decode(&bytes).expect("decodes"), capture);
    }

    /// **The layout is pinned by a committed file.** A change to what this
    /// build writes, or to how it reads version 1, fails here rather than
    /// leaving every capture recorded before it unreadable: the round trip
    /// above passes for any pair of a writer and a reader that agree with each
    /// other.
    #[test]
    fn the_committed_version_1_capture_reads_and_is_what_this_build_writes() {
        assert_eq!(Capture::decode(FIXTURE).expect("decodes"), every_field());
        assert_eq!(every_field().encode().expect("encodes"), FIXTURE);
        assert_eq!(&FIXTURE[..MAGIC.len()], MAGIC);
    }

    /// **Every prefix of a capture is refused as truncated**, and none panics:
    /// the reader never trusts a count or a length it has not got the bytes
    /// for.
    #[test]
    fn every_truncation_is_refused_and_none_panics() {
        for len in 0..FIXTURE.len() {
            let error = Capture::decode(&FIXTURE[..len]).expect_err("a prefix is not a capture");
            assert!(
                matches!(error, FormatError::Truncated(_)),
                "{len} bytes: {error:?}"
            );
        }
    }

    /// A count far past the file runs out of bytes rather than reserving
    /// memory for itself.
    #[test]
    fn a_count_larger_than_the_file_is_refused_as_truncated() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            Capture::decode(&bytes),
            Err(FormatError::Truncated("a CPU frame number"))
        );
    }

    /// The refusals that are not truncation, each from a one-byte change to a
    /// good capture.
    #[test]
    fn a_wrong_magic_a_later_version_an_undefined_tag_and_trailing_bytes_are_refused() {
        let mut magic = FIXTURE.to_vec();
        magic[0] = b'X';
        assert_eq!(Capture::decode(&magic), Err(FormatError::BadMagic));

        let mut later = FIXTURE.to_vec();
        later[MAGIC.len()..MAGIC.len() + 2].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
        assert_eq!(
            Capture::decode(&later),
            Err(FormatError::Newer {
                found: FORMAT_VERSION + 1,
                current: FORMAT_VERSION
            })
        );

        let mut trailing = FIXTURE.to_vec();
        trailing.push(0);
        assert_eq!(
            Capture::decode(&trailing),
            Err(FormatError::TrailingBytes(1))
        );

        // The first thread's name tag: after the magic, the version, the
        // environment and the CPU frame count, the frame number, the thread
        // count and the track.
        let environment: usize = every_field()
            .environment
            .entries
            .iter()
            .map(|(name, value)| 4 + name.len() + 4 + value.len())
            .sum();
        let tag_at = MAGIC.len() + 2 + 4 + environment + 4 + 8 + 4 + 4;
        assert_eq!(FIXTURE[tag_at], 1, "the offset names the name tag");
        let mut tag = FIXTURE.to_vec();
        tag[tag_at] = 2;
        assert_eq!(
            Capture::decode(&tag),
            Err(FormatError::BadTag {
                what: "a thread name",
                tag: 2
            })
        );
    }

    /// A statistics mask with a bit past the last counter is refused: it would
    /// otherwise read a counter the layout does not define, and every counter
    /// after it out of place.
    #[test]
    fn an_undefined_statistics_bit_is_refused() {
        let capture = Capture {
            gpu: vec![GpuFrame {
                number: 0,
                passes: vec![Pass {
                    label: String::new(),
                    begin_nanos: 0,
                    end_nanos: 0,
                    statistics: Some(Statistics::default()),
                }],
            }],
            ..Capture::default()
        };
        let mut bytes = capture.encode().expect("encodes");
        let mask_at = bytes.len() - 2;
        assert_eq!(&bytes[mask_at..], [0, 0], "an empty mask ends the file");
        bytes[mask_at + 1] = 0x04;
        assert_eq!(
            Capture::decode(&bytes),
            Err(FormatError::BadTag {
                what: "a statistics mask",
                tag: 0x0400
            })
        );
    }
}
