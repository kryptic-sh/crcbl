//! The Linux gamepad backend: evdev, polled, onto the gamepad seam.
//!
//! [`Evdev::new`] names `/dev/input`, and [`Evdev::poll`] reads every open pad
//! and hands what changed to a closure as [`GamepadEvent`]s — the same shape
//! as `xinput`, so a game consumes it the same way:
//!
//! ```no_run
//! # #[cfg(target_os = "linux")]
//! # fn main() -> Result<(), crcbl_input::evdev::EvdevError> {
//! use crcbl_input::{GamepadEvent, Stick};
//!
//! let mut pads = crcbl_input::evdev::Evdev::new();
//! // Once a frame, before the frame's input is read:
//! pads.poll(|event| {
//!     if let GamepadEvent::State { id, snapshot } = event {
//!         let (x, y) = snapshot.stick(Stick::Left); // raw, +Y up, no dead zone
//!         println!("pad {} left stick at ({x}, {y})", id.0);
//!     }
//! })?;
//! # Ok(())
//! # }
//! # #[cfg(not(target_os = "linux"))]
//! # fn main() {}
//! ```
//!
//! # Which nodes are pads
//!
//! Every `/dev/input/event*` node is opened read-only and non-blocking and
//! probed: `EVIOCGBIT` for its keys and axes, and it is a pad if it has
//! `BTN_GAMEPAD` and both `ABS_X` and `ABS_Y`. A node that is not a pad is
//! closed and not probed again while the same node stays in the directory. A
//! node that cannot be opened — a keyboard's is usually root-only, and a
//! pad's is too for the moment before udev grants the seat user access — is
//! skipped without an error and tried again at the next scan, since what cannot
//! be opened cannot be told apart from a pad. **A pad whose node the user
//! cannot read is therefore never found**: desktop distributions and SteamOS
//! grant it through udev's `uaccess` tag, which systemd's rules put on
//! joysticks.
//!
//! # What it reports
//!
//! - **Connected** when a pad node appears, with a fresh [`GamepadId`] and the
//!   [`PadKind`](crate::PadKind) its USB vendor and product name, then a **State** if the pad
//!   is not at rest (the probe reads what is held and where every axis is).
//! - **State** at each `SYN_REPORT` whose mapped snapshot differs from the
//!   last one reported — change-only, as `xinput` does and for the same
//!   reasons: the seam's snapshot is a level, and an identical one carries
//!   nothing.
//! - **Disconnected** when a read fails: `ENODEV` is the ordinary unplug and
//!   is not an error; anything else also returns an [`EvdevError`]. A pad
//!   plugged back in is a new node and a new id.
//!
//! After `SYN_DROPPED` — the kernel's buffer overflowed and events were lost —
//! everything up to the next `SYN_REPORT` is discarded and the pad's keys and
//! axes are read afresh, as the evdev docs prescribe.
//!
//! # New pads are found by re-scanning
//!
//! Open pads are read on every poll. `/dev/input` is listed again only once
//! [`RESCAN_INTERVAL`] has passed since the last scan, so a pad plugged in is
//! reported at most that late — the same trade as `xinput`'s re-probe, and
//! the same one second. inotify would report it at once, at the price of one
//! more descriptor and its own event parsing; nothing has needed it yet.
//!
//! # The mapping
//!
//! | evdev                                   | seam                                 |
//! | --------------------------------------- | ------------------------------------ |
//! | `BTN_SOUTH`, `BTN_EAST`                 | `South`, `East`                      |
//! | `BTN_NORTH` (`BTN_X`), `BTN_WEST` (`BTN_Y`) | `West`, `North` on Xbox-style drivers; `North`, `West` on Sony and Nintendo ones (below) |
//! | `BTN_TL`, `BTN_TR`                      | `LeftShoulder`, `RightShoulder`      |
//! | `BTN_THUMBL`, `BTN_THUMBR`              | `LeftStick`, `RightStick`            |
//! | `BTN_START`, `BTN_SELECT`, `BTN_MODE`   | `Start`, `Select`, `Guide`           |
//! | `BTN_DPAD_*`, or else `ABS_HAT0X`/`Y`   | `DpadUp`/`Down`/`Left`/`Right`       |
//! | `ABS_X`, `ABS_Y`, `ABS_RX`, `ABS_RY`    | the sticks, **Y negated** (evdev's +Y is down) |
//! | `ABS_Z` / `ABS_BRAKE` / `ABS_HAT2Y`     | `LeftTrigger`, first present; `BTN_TL2` if none |
//! | `ABS_RZ` / `ABS_GAS` / `ABS_HAT2X`      | `RightTrigger`, first present; `BTN_TR2` if none |
//!
//! Sticks are scaled by [`stick_axis`] and triggers by [`trigger_axis`], each
//! from the axis's own `EVIOCGABS` minimum and maximum. The driver's `flat` is
//! read but **not applied**: the seam's axes are raw and the dead zone is the
//! binding's (`gamepad.rs`).
//!
//! **The top and left face buttons depend on the driver**, the one place the
//! kernel disagrees with itself. The header spells `BTN_NORTH` and `BTN_X` as
//! one code, and `BTN_WEST` and `BTN_Y` as another; `hid-playstation`,
//! `hid-sony` and `hid-nintendo` send the positional names, while `xpad`,
//! Steam Input's virtual pad and `hid-steam` send the Xbox letters, so their
//! X — the left button — arrives as `BTN_NORTH`. The split is by USB vendor:
//! Sony and Nintendo positional, everything else lettered. Other drivers that
//! disagree are the "quirk zoo" the input design scopes out; the table
//! grows by demand.
//!
//! # Steam Input's virtual pad is read like any other
//!
//! Valve's vendor id, 0x28DE, is **not** filtered out. Without Steam Input,
//! Steam's gamepad emulation reaching this backend as an ordinary controller
//! is how a game launched from Steam on a Deck gets the built-in controls. That
//! emulated pad is the only copy: per `hid-steam`'s source, the driver
//! removes its own gamepad node while Steam holds the controller's hidraw
//! node, so one physical pad is not reported twice. Unverified on a Deck. A
//! game that turns Steam Input on (`crcbl-steam`'s `SteamPads`) would see the
//! pad twice, once from each, so `crcbl::engine::steam::steam_input` replaces
//! this backend rather than polling beside it; a vendor filter here is what
//! would let the two run together, and `docs/backlog.md` carries it.
//!
//! # Not on other targets
//!
//! This module is compiled on Linux (and into every target's tests, which is
//! how the mapping, the layouts and the poller are checked on the Windows and
//! macOS runners); only the device access is Linux-only. No other target gets
//! a stand-in that answers "no pads".

mod ffi;
#[cfg(target_os = "linux")]
mod linux;
mod map;

use crate::{GamepadEvent, GamepadId, GamepadSnapshot};
use ffi::{ABS_CNT, EV_ABS, EV_KEY, EV_SYN, InputEvent, KeyBits, SYN_DROPPED, SYN_REPORT};
#[cfg(target_os = "linux")]
pub use linux::Evdev;
use map::{Layout, Probe};
pub use map::{stick_axis, trigger_axis};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What went wrong reading evdev. Each variant carries the OS error's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvdevError {
    /// `/dev/input` could not be listed, so no new pad can be found. Open pads
    /// were still read.
    Scan {
        /// What the OS said.
        message: String,
    },
    /// A node opened but could not be probed, or failed to open for a reason
    /// other than permission or its having gone. It is tried again at the next
    /// scan.
    Open {
        /// The node.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },
    /// Reading an open pad failed with something other than `ENODEV`. The pad
    /// was reported as disconnected; its node is found again at a later scan
    /// if it is still there.
    Read {
        /// The node.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },
}

impl std::fmt::Display for EvdevError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scan { message } => write!(f, "could not list /dev/input: {message}"),
            Self::Open { path, message } => {
                write!(f, "could not probe {}: {message}", path.display())
            }
            Self::Read { path, message } => {
                write!(f, "could not read {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for EvdevError {}

/// How long `/dev/input` goes unlisted between scans for new pads.
///
/// A second, as `xinput`'s `REPROBE_INTERVAL` is and for its reason: a pad
/// takes a moment to enumerate anyway, and a scan opens and probes every node
/// it has not placed yet.
pub const RESCAN_INTERVAL: Duration = Duration::from_secs(1);

/// One directory entry that may be a device: its path, and its inode, which
/// changes when the kernel reuses the name for another device.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Node {
    path: PathBuf,
    ino: u64,
}

/// An open node the poller reads: the real `EventNode`, or a test's script.
trait Device {
    /// The device's ids, capabilities and current state.
    fn probe(&mut self) -> io::Result<Probe>;
    /// Appends every event waiting on the node, returning once none is left.
    fn read(&mut self, events: &mut Vec<InputEvent>) -> io::Result<()>;
}

/// Where the poller finds nodes: `/dev/input`, or a test's script.
trait Source {
    /// The open node type.
    type Device: Device;
    /// Every node that may be a device, now.
    fn scan(&mut self) -> io::Result<Vec<Node>>;
    /// Opens one.
    fn open(&mut self, path: &Path) -> io::Result<Self::Device>;
}

/// Whether a failed open is the ordinary kind a scan skips without an error:
/// a node the user may not read, or one gone between the listing and the open.
fn skipped_quietly(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::NotFound
    ) || error.raw_os_error() == Some(ffi::ENODEV)
}

/// A connected pad.
#[derive(Debug)]
struct Pad<D> {
    node: Node,
    device: D,
    id: GamepadId,
    layout: Layout,
    /// The keys held, as far as the events read so far say.
    held: KeyBits,
    /// Every axis's value, likewise.
    values: [i32; ABS_CNT],
    /// Between a `SYN_DROPPED` and the `SYN_REPORT` that ends it.
    dropping: bool,
    /// The snapshot last reported.
    last: GamepadSnapshot,
}

impl<D: Device> Pad<D> {
    /// Takes the state `probe` read as the pad's own.
    fn take_state(&mut self, probe: &Probe) {
        self.held = probe.held;
        self.values = probe.values();
    }

    /// Emits a State if the snapshot differs from the last one reported.
    fn report(&mut self, emit: &mut impl FnMut(GamepadEvent)) {
        let snapshot = self.layout.snapshot(&self.held, &self.values);
        if snapshot != self.last {
            self.last = snapshot;
            emit(GamepadEvent::State {
                id: self.id,
                snapshot,
            });
        }
    }

    /// Reads every waiting event and reports at each `SYN_REPORT`.
    fn pump(
        &mut self,
        events: &mut Vec<InputEvent>,
        emit: &mut impl FnMut(GamepadEvent),
    ) -> io::Result<()> {
        events.clear();
        self.device.read(events)?;
        for event in events.iter() {
            match (event.kind, event.code) {
                (EV_SYN, SYN_DROPPED) => self.dropping = true,
                (EV_SYN, SYN_REPORT) => {
                    if self.dropping {
                        let probe = self.device.probe()?;
                        self.take_state(&probe);
                        self.dropping = false;
                    }
                    self.report(emit);
                }
                _ if self.dropping => {}
                (EV_KEY, code) => self.held.set(code, event.value != 0),
                (EV_ABS, code) => {
                    if let Some(value) = self.values.get_mut(usize::from(code)) {
                        *value = event.value;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// The open pads, the nodes known not to be pads, and the scan clock.
#[derive(Debug)]
struct Poller<D> {
    pads: Vec<Pad<D>>,
    /// Nodes probed and found not to be pads, forgotten once they leave the
    /// directory so a reused name is probed afresh.
    not_pads: HashSet<Node>,
    /// When `/dev/input` was last listed.
    scanned: Option<Instant>,
    /// What went wrong at the last scan, returned by every poll until the
    /// next scan says otherwise — so a node that keeps failing its probe
    /// reads as one lasting failure, not one that clears between scans.
    scan_failure: Option<EvdevError>,
    /// The read buffer, kept between polls.
    events: Vec<InputEvent>,
}

impl<D> Default for Poller<D> {
    fn default() -> Self {
        Self {
            pads: Vec::new(),
            not_pads: HashSet::new(),
            scanned: None,
            scan_failure: None,
            events: Vec::new(),
        }
    }
}

impl<D: Device> Poller<D> {
    /// Reads every open pad, then scans for new ones if the interval has
    /// passed, emitting what changed — see the module docs. `now` is the
    /// caller's, so a test can step it.
    fn poll<S: Source<Device = D>>(
        &mut self,
        source: &mut S,
        now: Instant,
        emit: &mut impl FnMut(GamepadEvent),
    ) -> Result<(), EvdevError> {
        let mut failure = None;
        let events = &mut self.events;
        self.pads.retain_mut(|pad| match pad.pump(events, emit) {
            Ok(()) => true,
            Err(error) => {
                emit(GamepadEvent::Disconnected { id: pad.id });
                if error.raw_os_error() != Some(ffi::ENODEV) {
                    failure = Some(EvdevError::Read {
                        path: pad.node.path.clone(),
                        message: error.to_string(),
                    });
                }
                false
            }
        });

        if self
            .scanned
            .is_none_or(|at| now.duration_since(at) >= RESCAN_INTERVAL)
        {
            self.scanned = Some(now);
            self.scan_failure = None;
            match source.scan() {
                Ok(nodes) => {
                    self.not_pads.retain(|node| nodes.contains(node));
                    for node in nodes {
                        if let Err(error) = self.connect(source, node, emit) {
                            self.scan_failure = Some(error);
                        }
                    }
                }
                Err(error) => {
                    self.scan_failure = Some(EvdevError::Scan {
                        message: error.to_string(),
                    });
                }
            }
        }
        match failure.or_else(|| self.scan_failure.clone()) {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }

    /// Opens and probes `node` unless it is already placed, and connects it
    /// if it is a pad.
    fn connect<S: Source<Device = D>>(
        &mut self,
        source: &mut S,
        node: Node,
        emit: &mut impl FnMut(GamepadEvent),
    ) -> Result<(), EvdevError> {
        if self.not_pads.contains(&node) || self.pads.iter().any(|pad| pad.node == node) {
            return Ok(());
        }
        let opened = source.open(&node.path).and_then(|mut device| {
            let probe = device.probe()?;
            Ok((device, probe))
        });
        let (device, probe) = match opened {
            Ok(opened) => opened,
            Err(error) if skipped_quietly(&error) => return Ok(()),
            Err(error) => {
                return Err(EvdevError::Open {
                    path: node.path,
                    message: error.to_string(),
                });
            }
        };
        if !map::is_gamepad(&probe.keys, &probe.abs) {
            self.not_pads.insert(node);
            return Ok(());
        }
        let layout = Layout::new(&probe);
        let id = GamepadId::allocate();
        let kind = layout.kind;
        emit(GamepadEvent::Connected { id, kind });
        let mut pad = Pad {
            node,
            device,
            id,
            layout,
            held: KeyBits::EMPTY,
            values: [0; ABS_CNT],
            dropping: false,
            last: GamepadSnapshot::neutral(kind),
        };
        pad.take_state(&probe);
        pad.report(emit);
        self.pads.push(pad);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionKind, ActionMap, Binding, PadAxis, PadButton, PadKind};
    use ffi::{ABS_X, ABS_Y, BTN_EAST, BTN_SOUTH, ENODEV};
    use map::tests::{XPAD_STICK, xpad};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    /// One scripted node: what a probe answers, the events waiting on it, and
    /// the errors it is set to fail with.
    #[derive(Debug)]
    struct Script {
        probe: Probe,
        pending: Vec<InputEvent>,
        open_error: Option<io::ErrorKind>,
        read_error: Option<i32>,
        probes: u32,
    }

    type Shared = Rc<RefCell<Script>>;

    #[derive(Debug)]
    struct FakeDevice(Shared);

    impl Device for FakeDevice {
        fn probe(&mut self) -> io::Result<Probe> {
            let mut script = self.0.borrow_mut();
            script.probes += 1;
            Ok(script.probe.clone())
        }

        fn read(&mut self, events: &mut Vec<InputEvent>) -> io::Result<()> {
            let mut script = self.0.borrow_mut();
            if let Some(code) = script.read_error {
                return Err(io::Error::from_raw_os_error(code));
            }
            events.append(&mut script.pending);
            Ok(())
        }
    }

    /// A directory of scripted nodes, by name and inode, counting scans and
    /// opens.
    #[derive(Debug, Default)]
    struct Fake {
        nodes: BTreeMap<PathBuf, (u64, Shared)>,
        scans: u32,
        opens: u32,
        now: Option<Instant>,
    }

    impl Fake {
        /// Puts a node with `probe` at `/dev/input/{name}` with inode `ino`,
        /// returning its script.
        fn plug(&mut self, name: &str, ino: u64, probe: Probe) -> Shared {
            let script = Rc::new(RefCell::new(Script {
                probe,
                pending: Vec::new(),
                open_error: None,
                read_error: None,
                probes: 0,
            }));
            self.nodes.insert(
                Path::new("/dev/input").join(name),
                (ino, Rc::clone(&script)),
            );
            script
        }

        fn unplug(&mut self, name: &str) {
            self.nodes.remove(&Path::new("/dev/input").join(name));
        }
    }

    impl Source for Fake {
        type Device = FakeDevice;

        fn scan(&mut self) -> io::Result<Vec<Node>> {
            self.scans += 1;
            Ok(self
                .nodes
                .iter()
                .map(|(path, (ino, _))| Node {
                    path: path.clone(),
                    ino: *ino,
                })
                .collect())
        }

        fn open(&mut self, path: &Path) -> io::Result<FakeDevice> {
            self.opens += 1;
            let (_, script) = self.nodes.get(path).ok_or(io::ErrorKind::NotFound)?;
            if let Some(kind) = script.borrow().open_error {
                return Err(kind.into());
            }
            Ok(FakeDevice(Rc::clone(script)))
        }
    }

    /// Polls `after` the fake's clock, moving the clock there.
    fn poll_after(
        poller: &mut Poller<FakeDevice>,
        fake: &mut Fake,
        after: Duration,
    ) -> (Vec<GamepadEvent>, Result<(), EvdevError>) {
        let now = fake.now.map_or_else(Instant::now, |now| now + after);
        fake.now = Some(now);
        let mut events = Vec::new();
        let result = poller.poll(fake, now, &mut |event| events.push(event));
        (events, result)
    }

    /// Polls a whole scan interval after the last poll, so a scan runs.
    fn poll(
        poller: &mut Poller<FakeDevice>,
        fake: &mut Fake,
    ) -> (Vec<GamepadEvent>, Result<(), EvdevError>) {
        poll_after(poller, fake, RESCAN_INTERVAL)
    }

    const fn key(code: u16, value: i32) -> InputEvent {
        InputEvent::new(EV_KEY, code, value)
    }
    const fn abs(code: u16, value: i32) -> InputEvent {
        InputEvent::new(EV_ABS, code, value)
    }
    const REPORT: InputEvent = InputEvent::new(EV_SYN, SYN_REPORT, 0);
    const DROPPED: InputEvent = InputEvent::new(EV_SYN, SYN_DROPPED, 0);

    /// A keyboard: keys, no pad button, no axes.
    fn keyboard() -> Probe {
        map::tests::probe(map::tests::XBOX, &[1, 30, 57], &[])
    }

    /// **Connect, change, disconnect on `ENODEV`, reconnect**, each reported
    /// once and only when it happens, with a fresh id for the new node.
    #[test]
    fn transitions_are_reported_once_each() {
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        assert_eq!(poll(&mut poller, &mut fake), (vec![], Ok(())), "no nodes");

        let pad = fake.plug("event3", 10, xpad());
        let (events, result) = poll(&mut poller, &mut fake);
        assert_eq!(result, Ok(()));
        let [GamepadEvent::Connected { id, kind }] = events[..] else {
            panic!("a pad at rest connects with no state: {events:?}");
        };
        assert_eq!(kind, PadKind::Xbox);

        pad.borrow_mut().pending = vec![key(BTN_SOUTH, 1), abs(ABS_Y, -32768), REPORT];
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        let [GamepadEvent::State { id: same, snapshot }] = events[..] else {
            panic!("one report, one state: {events:?}");
        };
        assert_eq!(same, id);
        assert!(snapshot.buttons.contains(PadButton::South));
        assert_eq!(snapshot.axis(PadAxis::LeftY), 1.0, "pushed up");

        pad.borrow_mut().pending = vec![key(BTN_SOUTH, 2), REPORT];
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        assert!(events.is_empty(), "autorepeat changes nothing: {events:?}");

        pad.borrow_mut().pending = vec![key(BTN_SOUTH, 0), abs(ABS_Y, 0)];
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        assert!(
            events.is_empty(),
            "nothing before the SYN_REPORT: {events:?}"
        );
        pad.borrow_mut().pending = vec![REPORT];
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        assert_eq!(
            events,
            [GamepadEvent::State {
                id,
                snapshot: GamepadSnapshot::neutral(PadKind::Xbox),
            }]
        );

        pad.borrow_mut().read_error = Some(ENODEV);
        fake.unplug("event3");
        assert_eq!(
            poll(&mut poller, &mut fake),
            (vec![GamepadEvent::Disconnected { id }], Ok(())),
            "ENODEV is an unplug, not an error"
        );

        fake.plug("event3", 11, xpad());
        let (events, _) = poll(&mut poller, &mut fake);
        let [GamepadEvent::Connected { id: again, .. }] = events[..] else {
            panic!("{events:?}");
        };
        assert_ne!(again, id, "a new node is a new pad");
    }

    /// A pad that is not at rest when it connects says so at once.
    #[test]
    fn a_pad_connected_mid_press_reports_its_state() {
        let mut held = xpad();
        held.held.set(BTN_EAST, true);
        held.axes[usize::from(ABS_X)].value = XPAD_STICK.1;
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        fake.plug("event0", 1, held);
        let (events, _) = poll(&mut poller, &mut fake);
        let [
            GamepadEvent::Connected { id, .. },
            GamepadEvent::State { id: same, snapshot },
        ] = events[..]
        else {
            panic!("{events:?}");
        };
        assert_eq!(same, id);
        assert!(snapshot.buttons.contains(PadButton::East));
        assert_eq!(snapshot.axis(PadAxis::LeftX), 1.0);
    }

    /// Any read error but `ENODEV` disconnects the pad **and** is returned,
    /// and the other pads are still read.
    #[test]
    fn an_unexpected_read_error_disconnects_and_is_reported() {
        const EIO: i32 = 5;
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        let broken = fake.plug("event0", 1, xpad());
        let fine = fake.plug("event1", 2, xpad());
        let (events, _) = poll(&mut poller, &mut fake);
        let [
            GamepadEvent::Connected { id: first, .. },
            GamepadEvent::Connected { id: second, .. },
        ] = events[..]
        else {
            panic!("{events:?}");
        };

        broken.borrow_mut().read_error = Some(EIO);
        fine.borrow_mut().pending = vec![key(BTN_SOUTH, 1), REPORT];
        let (events, result) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        assert_eq!(events[0], GamepadEvent::Disconnected { id: first });
        assert!(
            matches!(events[1], GamepadEvent::State { id, .. } if id == second),
            "{events:?}"
        );
        let Err(EvdevError::Read { path, .. }) = result else {
            panic!("{result:?}");
        };
        assert_eq!(path, Path::new("/dev/input/event0"));
    }

    /// **A keyboard is probed once and then left alone** while its node
    /// stays; a pad beside it connects.
    #[test]
    fn a_keyboard_is_not_a_pad_and_is_not_reopened() {
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        let keys = fake.plug("event0", 1, keyboard());
        fake.plug("event1", 2, xpad());
        let (events, _) = poll(&mut poller, &mut fake);
        assert!(
            matches!(events[..], [GamepadEvent::Connected { .. }]),
            "only the pad: {events:?}"
        );
        assert_eq!(fake.opens, 2);
        for _ in 0..3 {
            poll(&mut poller, &mut fake).1.expect("scripted");
        }
        assert_eq!(fake.scans, 4);
        assert_eq!(fake.opens, 2, "neither node is opened again");
        assert_eq!(keys.borrow().probes, 1);
    }

    /// A node name reused by a different device — a new inode — is probed
    /// afresh, so a pad plugged into a keyboard's old name is found.
    #[test]
    fn a_reused_node_name_is_probed_again() {
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        fake.plug("event0", 1, keyboard());
        assert_eq!(poll(&mut poller, &mut fake).0, []);
        fake.plug("event0", 2, xpad());
        let (events, _) = poll(&mut poller, &mut fake);
        assert!(
            matches!(events[..], [GamepadEvent::Connected { .. }]),
            "{events:?}"
        );
    }

    /// **A node the user may not open is skipped with no error**, and tried
    /// again at every scan, so a pad whose permissions arrive a moment late
    /// is found. Any other open failure is returned.
    #[test]
    fn an_unreadable_node_is_skipped_and_retried() {
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        let pad = fake.plug("event0", 1, xpad());
        pad.borrow_mut().open_error = Some(io::ErrorKind::PermissionDenied);
        assert_eq!(poll(&mut poller, &mut fake), (vec![], Ok(())));
        assert_eq!(poll(&mut poller, &mut fake), (vec![], Ok(())));
        assert_eq!(fake.opens, 2, "retried at the second scan");

        pad.borrow_mut().open_error = Some(io::ErrorKind::OutOfMemory);
        let (events, result) = poll(&mut poller, &mut fake);
        assert!(events.is_empty());
        assert!(matches!(result, Err(EvdevError::Open { .. })), "{result:?}");
        let (_, between) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        assert_eq!(between, result, "still failing between scans");

        pad.borrow_mut().open_error = None;
        let (events, result) = poll(&mut poller, &mut fake);
        assert_eq!(result, Ok(()));
        assert!(matches!(events[..], [GamepadEvent::Connected { .. }]));
    }

    /// **`/dev/input` is re-listed only once the interval has passed**, while
    /// an open pad is read on every poll.
    #[test]
    fn scans_are_throttled_and_pads_are_read_every_poll() {
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        let pad = fake.plug("event0", 1, xpad());
        poll(&mut poller, &mut fake).1.expect("scripted");
        assert_eq!(fake.scans, 1);

        fake.plug("event1", 2, xpad());
        let almost = RESCAN_INTERVAL - Duration::from_millis(1);
        pad.borrow_mut().pending = vec![key(BTN_SOUTH, 1), REPORT];
        let (events, _) = poll_after(&mut poller, &mut fake, almost);
        assert!(
            matches!(events[..], [GamepadEvent::State { .. }]),
            "the open pad is read, the new node not yet seen: {events:?}"
        );
        assert_eq!(fake.scans, 1);

        let (events, _) = poll_after(&mut poller, &mut fake, Duration::from_millis(1));
        assert_eq!(fake.scans, 2, "scanned once the interval passed");
        assert!(
            matches!(events[..], [GamepadEvent::Connected { .. }]),
            "{events:?}"
        );
    }

    /// **After `SYN_DROPPED`**, events up to the next `SYN_REPORT` are
    /// discarded and the pad's state is read afresh from the device.
    #[test]
    fn a_dropped_buffer_resynchronises_from_the_device() {
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        let pad = fake.plug("event0", 1, xpad());
        let (events, _) = poll(&mut poller, &mut fake);
        let [GamepadEvent::Connected { id, .. }] = events[..] else {
            panic!("{events:?}");
        };

        // The truth after the overflow: East held, not South.
        pad.borrow_mut().probe.held.set(BTN_EAST, true);
        pad.borrow_mut().pending = vec![DROPPED, key(BTN_SOUTH, 1), REPORT];
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        let [GamepadEvent::State { id: same, snapshot }] = events[..] else {
            panic!("{events:?}");
        };
        assert_eq!(same, id);
        assert!(snapshot.buttons.contains(PadButton::East), "re-read");
        assert!(!snapshot.buttons.contains(PadButton::South), "discarded");
        assert_eq!(
            pad.borrow().probes,
            2,
            "one probe to connect, one to resync"
        );

        pad.borrow_mut().pending = vec![key(BTN_SOUTH, 1), REPORT];
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        let [GamepadEvent::State { snapshot, .. }] = events[..] else {
            panic!("{events:?}");
        };
        assert!(
            snapshot.buttons.contains(PadButton::South),
            "read normally again"
        );
    }

    /// **The poller's events drive a binding**, and an unplug releases it.
    #[test]
    fn polled_events_drive_an_action_map() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "jump".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::PadButton(PadButton::South)],
        });
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        let pad = fake.plug("event0", 1, xpad());
        pad.borrow_mut().probe.held.set(BTN_SOUTH, true);
        let now = Instant::now();
        poller
            .poll(&mut fake, now, &mut |event| map.gamepad_event(&event))
            .expect("scripted");
        assert!(map.just_pressed("jump"));

        pad.borrow_mut().read_error = Some(ENODEV);
        poller
            .poll(&mut fake, now, &mut |event| map.gamepad_event(&event))
            .expect("scripted");
        assert!(map.just_released("jump"), "unplugging releases");
    }

    #[test]
    fn errors_name_what_failed() {
        let error = EvdevError::Read {
            path: PathBuf::from("/dev/input/event4"),
            message: "Input/output error".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "could not read /dev/input/event4: Input/output error"
        );
    }
}
