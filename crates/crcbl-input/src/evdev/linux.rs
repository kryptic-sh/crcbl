//! The real device source: `/dev/input/event*` nodes, opened non-blocking,
//! probed with `EVIOCG*` ioctls, and read as runs of `input_event`.

use super::ffi::{
    ABS_CNT, AbsBits, AbsInfo, EV_ABS, EV_KEY, EVENT_SIZE, EVIOCGID_NR, EVIOCGKEY_NR, InputEvent,
    InputId, KernelUlong, KeyBits, O_NONBLOCK, decode_events, evioc_read, eviocgabs_nr,
    eviocgbit_nr, ioctl, ioctl_request,
};
use super::map::Probe;
use super::{Device, Node, Poller, Source};
use crate::GamepadEvent;
use crate::evdev::EvdevError;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirEntryExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Where the kernel's input nodes are.
const INPUT_DIR: &str = "/dev/input";

/// Events read per `read` call.
const READ_BATCH: usize = 64;

/// A type the kernel may fill with any bytes.
///
/// # Safety
/// Every bit pattern of the type's size must be a valid value of it: plain
/// integers, or `repr(C)` structures of them with no padding.
unsafe trait Plain {}
// SAFETY: four `u16`s, no padding (the layout test).
unsafe impl Plain for InputId {}
// SAFETY: six `i32`s, no padding (the layout test).
unsafe impl Plain for AbsInfo {}
// SAFETY: an array of integers.
unsafe impl<const WORDS: usize> Plain for [KernelUlong; WORDS] {}

/// Runs the evdev request `nr` on `file`, with the kernel writing into `out`.
///
/// The request's size field is `size_of::<T>()`, so the size the kernel is
/// told is the size of `out` by construction, not by a caller's care.
fn read_ioctl<T: Plain>(file: &File, nr: u32, out: &mut T) -> io::Result<()> {
    let request = ioctl_request(evioc_read(nr, size_of::<T>()));
    // SAFETY: `file` is open for the call. Every `EVIOCG*` request copies at
    // most the request's size field of bytes to the pointer, and that field
    // is `size_of::<T>()`, the size of the exclusive borrow `out`; `T: Plain`
    // makes any bytes the kernel writes a valid `T`. A node that is not an
    // evdev device answers `ENOTTY` and writes nothing.
    let result = unsafe {
        ioctl(
            file.as_raw_fd(),
            request,
            core::ptr::from_mut(out).cast::<core::ffi::c_void>(),
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// `/dev/input`, or a test's stand-in directory.
#[derive(Debug)]
struct InputDir {
    dir: PathBuf,
}

impl Source for InputDir {
    type Device = EventNode;

    fn scan(&mut self) -> io::Result<Vec<Node>> {
        let mut nodes = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            if entry.file_name().as_encoded_bytes().starts_with(b"event") {
                nodes.push(Node {
                    path: entry.path(),
                    ino: entry.ino(),
                });
            }
        }
        nodes.sort();
        Ok(nodes)
    }

    fn open(&mut self, path: &Path) -> io::Result<EventNode> {
        // `std` adds `O_CLOEXEC` itself.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(path)?;
        Ok(EventNode { file })
    }
}

/// One open event node.
#[derive(Debug)]
struct EventNode {
    file: File,
}

impl Device for EventNode {
    fn probe(&mut self) -> io::Result<Probe> {
        let mut probe = Probe {
            id: InputId::default(),
            keys: KeyBits::EMPTY,
            abs: AbsBits::EMPTY,
            held: KeyBits::EMPTY,
            axes: [AbsInfo::default(); ABS_CNT],
        };
        read_ioctl(&self.file, EVIOCGID_NR, &mut probe.id)?;
        read_ioctl(&self.file, eviocgbit_nr(EV_KEY), &mut probe.keys.0)?;
        read_ioctl(&self.file, eviocgbit_nr(EV_ABS), &mut probe.abs.0)?;
        read_ioctl(&self.file, EVIOCGKEY_NR, &mut probe.held.0)?;
        for code in (0..ABS_CNT as u16).filter(|&code| probe.abs.has(code)) {
            read_ioctl(
                &self.file,
                eviocgabs_nr(code),
                &mut probe.axes[usize::from(code)],
            )?;
        }
        Ok(probe)
    }

    fn read(&mut self, events: &mut Vec<InputEvent>) -> io::Result<()> {
        let mut buffer = [0; READ_BATCH * EVENT_SIZE];
        loop {
            match self.file.read(&mut buffer) {
                Ok(read) => {
                    events.extend(decode_events(&buffer[..read])?);
                    if read < buffer.len() {
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// The evdev pads on this machine, and their connection state.
#[derive(Debug)]
pub struct Evdev {
    source: InputDir,
    poller: Poller<EventNode>,
}

impl Evdev {
    /// Pads under `/dev/input`, none of them found yet: the first
    /// [`poll`](Self::poll) scans.
    #[must_use]
    pub fn new() -> Self {
        Self {
            source: InputDir {
                dir: PathBuf::from(INPUT_DIR),
            },
            poller: Poller::default(),
        }
    }

    /// Reads every open pad and calls `emit` with what changed: connections,
    /// disconnections, and snapshots that differ from the last one reported.
    /// Open pads are read on every call; `/dev/input` is re-scanned for new
    /// ones only once [`RESCAN_INTERVAL`](super::RESCAN_INTERVAL) has passed
    /// since the last scan.
    ///
    /// # Errors
    /// An [`EvdevError`] for the last thing that went wrong unexpectedly; see
    /// its variants. Everything else was still polled.
    pub fn poll(&mut self, mut emit: impl FnMut(GamepadEvent)) -> Result<(), EvdevError> {
        self.poller
            .poll(&mut self.source, Instant::now(), &mut emit)
    }
}

impl Default for Evdev {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evdev::ffi::{ABS_Y, EV_SYN, SYN_REPORT};

    /// A fresh directory under the system temp dir, removed by name on drop.
    struct Scratch {
        dir: PathBuf,
        files: Vec<PathBuf>,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("crcbl-evdev-{name}-{}", std::process::id()));
            fs::create_dir(&dir).expect("a fresh scratch dir");
            Self {
                dir,
                files: Vec::new(),
            }
        }

        fn file(&mut self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.dir.join(name);
            fs::write(&path, bytes).expect("scratch file written");
            self.files.push(path.clone());
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            for file in &self.files {
                fs::remove_file(file).expect("scratch file removed");
            }
            fs::remove_dir(&self.dir).expect("scratch dir removed");
        }
    }

    /// **A scan lists the `event*` nodes and nothing else**, sorted, each
    /// with its inode.
    #[test]
    fn a_scan_lists_only_event_nodes() {
        let mut scratch = Scratch::new("scan");
        let event1 = scratch.file("event1", b"");
        scratch.file("js0", b"");
        scratch.file("mouse0", b"");
        let event0 = scratch.file("event0", b"");
        let mut source = InputDir {
            dir: scratch.dir.clone(),
        };
        let nodes = source.scan().expect("a readable dir");
        let paths: Vec<_> = nodes.iter().map(|node| node.path.clone()).collect();
        assert_eq!(paths, [event0, event1]);
        assert_ne!(nodes[0].ino, nodes[1].ino);
    }

    /// **The real `ioctl` answers a node that is not an evdev device with
    /// `ENOTTY`**, and the poller reports that as an error rather than a pad
    /// or silence.
    #[test]
    fn a_non_evdev_node_fails_its_probe_with_enotty() {
        const ENOTTY: i32 = 25;
        let mut scratch = Scratch::new("probe");
        let path = scratch.file("event0", b"");
        let mut source = InputDir {
            dir: scratch.dir.clone(),
        };
        let mut node = source.open(&path).expect("a regular file opens");
        let error = node.probe().expect_err("a regular file has no EVIOCGID");
        assert_eq!(error.raw_os_error(), Some(ENOTTY), "{error}");

        let mut events = Vec::new();
        let result = Poller::default().poll(&mut source, Instant::now(), &mut |event| {
            events.push(event);
        });
        assert!(
            matches!(&result, Err(EvdevError::Open { path: failed, .. }) if *failed == path),
            "{result:?}"
        );
        assert!(events.is_empty());
    }

    /// **The real `read` path decodes what the kernel's layout puts down**:
    /// a regular file holding two events, read through `EventNode`.
    #[test]
    fn a_read_decodes_whole_events() {
        let mut bytes = Vec::new();
        for event in [
            InputEvent::new(EV_ABS, ABS_Y, 32767),
            InputEvent::new(EV_SYN, SYN_REPORT, 0),
        ] {
            bytes.extend_from_slice(&[0; 2 * size_of::<KernelUlong>()]);
            bytes.extend_from_slice(&event.kind.to_ne_bytes());
            bytes.extend_from_slice(&event.code.to_ne_bytes());
            bytes.extend_from_slice(&event.value.to_ne_bytes());
        }
        let mut scratch = Scratch::new("read");
        let path = scratch.file("event0", &bytes);
        let mut node = InputDir {
            dir: scratch.dir.clone(),
        }
        .open(&path)
        .expect("a regular file opens");
        let mut events = Vec::new();
        node.read(&mut events).expect("whole events");
        assert_eq!(
            events,
            [
                InputEvent::new(EV_ABS, ABS_Y, 32767),
                InputEvent::new(EV_SYN, SYN_REPORT, 0),
            ]
        );
    }
}
