//! The browser gamepad backend: the Web Gamepad API, polled, onto the gamepad
//! seam.
//!
//! [`WebGamepads::poll`] reads the pads the page's shim reported this frame
//! and hands what changed to a closure as [`GamepadEvent`]s — the same shape
//! as `xinput` and `evdev`, so a game consumes it the same way:
//!
//! ```no_run
//! # #[cfg(target_arch = "wasm32")]
//! # fn main() {
//! use crcbl_input::{GamepadEvent, Stick};
//!
//! let mut pads = crcbl_input::web_gamepad::WebGamepads::new();
//! // Once a frame, after the shim's pump and before the frame's input is read:
//! let unmapped = pads.poll(|event| {
//!     if let GamepadEvent::State { id, snapshot } = event {
//!         let (x, y) = snapshot.stick(Stick::Left); // raw, +Y up, no dead zone
//!         println!("pad {} left stick at ({x}, {y})", id.0);
//!     }
//! });
//! for pad in unmapped {
//!     println!("{pad}");
//! }
//! # }
//! # #[cfg(not(target_arch = "wasm32"))]
//! # fn main() {}
//! ```
//!
//! # The shim reads the pads, once a frame
//!
//! The Gamepad API is poll-based: `navigator.getGamepads()` is a snapshot, and
//! nothing fires when a stick moves. And wasm here imports nothing (see
//! `crcbl::web`), so it cannot call the API itself. `web/engine/gamepad.js`
//! therefore calls it once per `requestAnimationFrame`, before the demo's
//! frame, and writes every pad into buffers wasm owns through the entry points
//! in [`shim`] — which is also where the layout of one report is specified.
//!
//! **A browser shows no pad until one of its buttons is pressed on the page.**
//! Chrome, Firefox and Safari all hide pads from `getGamepads()` until a
//! button press while the page is visible (a fingerprinting defence), so a
//! pad already plugged in when the demo opens connects on its first press and
//! not before; moving a stick does not count. The API also needs a secure
//! context — `https:` or `localhost` — and a `gamepad` permissions policy that
//! allows it; without either, the shim reports nothing and there are no pads.
//!
//! # What it reports
//!
//! - **Connected** when an index first reports a connected, standard-mapped
//!   pad, with a fresh [`GamepadId`] and the [`PadKind`] its
//!   `Gamepad.id` names (the USB ids in Chrome's or Firefox's form, else the
//!   product name). Then a **State** if the pad is not at rest.
//! - **State** when a pad's mapped snapshot differs from the last one
//!   reported — change-only, as `xinput` and `evdev` are, and for the same
//!   reasons.
//! - **Disconnected** when an index stops reporting, reports
//!   `connected: false`, or reports a different `Gamepad.id` or mapping. The
//!   last case is a new pad at a reused index — a browser hands a freed index
//!   to the next pad — and reads as a disconnect and a connect. A pad
//!   unplugged and plugged back in between two frames at the same index with
//!   the same id is not seen to leave; nothing in a snapshot tells it apart.
//!
//! No `gamepadconnected`/`gamepaddisconnected` listener: `getGamepads()` is
//! read every frame anyway, so the events would only say one frame early what
//! the poll says, and a missed one would be one more way to disagree with it.
//!
//! # The mapping
//!
//! Only `Gamepad.mapping === "standard"` is mapped, the layout the W3C spec
//! fixes (the "Remapping" section's figure):
//!
//! | standard mapping                        | seam                                   |
//! | --------------------------------------- | -------------------------------------- |
//! | `buttons[0]`…`[3]`                      | `South`, `East`, `West`, `North` (positional) |
//! | `buttons[4]`, `[5]`                     | `LeftShoulder`, `RightShoulder`        |
//! | `buttons[6]`, `[7]`, by `value`         | `LeftTrigger`, `RightTrigger`, 0…1     |
//! | `buttons[8]`, `[9]`                     | `Select`, `Start`                      |
//! | `buttons[10]`, `[11]`                   | `LeftStick`, `RightStick`              |
//! | `buttons[12]`…`[15]`                    | `DpadUp`, `DpadDown`, `DpadLeft`, `DpadRight` |
//! | `buttons[16]`                           | `Guide`                                |
//! | `axes[0]`, `axes[1]`                    | the left stick, **Y negated** (the spec's +Y is down) |
//! | `axes[2]`, `axes[3]`                    | the right stick, likewise              |
//!
//! Buttons are read from `pressed`, triggers from `value`. Sticks and triggers
//! are clamped to their ranges ([`stick_axis`], [`trigger_axis`]) and a
//! non-finite value reads as rest; **no dead zone** is applied, as the seam
//! requires. Buttons past 16 and axes past 3 (a touchpad click, a second
//! stick's extras) are not read.
//!
//! # A pad without the standard mapping is skipped, and said once
//!
//! `mapping === ""` means the browser does not know the pad's layout, and its
//! button and axis indices are then whatever the OS driver's order happens to
//! be: a best-effort guess would put `South` on a different button for every
//! pad, browser and OS, and a binding that fires the wrong button is worse
//! than none. So such a pad is **not connected at all** — no event, no id —
//! and [`WebGamepads::poll`] returns it as an [`Unmapped`] once, on the frame
//! it appears, for the caller to log. It is returned again only if it leaves
//! and comes back. Chrome and Firefox standard-map the common Xbox,
//! PlayStation and Switch pads; the input design scopes a quirk table for the
//! rest out (`docs/notes/simulation.md`).
//!
//! # Not on other targets
//!
//! This module is compiled on `wasm32` (and into every target's tests, which is
//! how the mapping, the poller and the entry points are checked on the native
//! runners); the entry points are exported only from a `wasm32` build. No
//! other target gets a stand-in that answers "no pads".

mod map;
pub mod shim;

use crate::{GamepadEvent, GamepadId, GamepadSnapshot, PadKind};
pub use map::{STANDARD_AXES, STANDARD_BUTTONS, VALUES, stick_axis, trigger_axis};

/// How many bytes of `Gamepad.id` a report carries. The shim sends a longer id
/// as an empty one, which costs only the family's name ([`PadKind::Generic`]);
/// real ids are a fraction of this.
pub const ID_CAPACITY: usize = 256;

/// One entry of `navigator.getGamepads()`, as the shim reported it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Report {
    /// `Gamepad.index`.
    index: u32,
    /// `Gamepad.connected`.
    connected: bool,
    /// `Gamepad.mapping === "standard"`.
    standard: bool,
    /// `buttons[i].pressed` in bit `i`.
    pressed: u32,
    /// Every standard button's `value`, then every standard axis.
    values: [f32; VALUES],
    /// `Gamepad.id`, UTF-8, in its first `id_len` bytes.
    id: [u8; ID_CAPACITY],
    id_len: usize,
}

impl Report {
    /// The id's bytes.
    fn id(&self) -> &[u8] {
        &self.id[..self.id_len]
    }

    /// The id as text, for a log line and the family's name.
    fn name(&self) -> String {
        String::from_utf8_lossy(self.id()).into_owned()
    }
}

/// Where the poller takes a frame of reports from: the shim's, or a test's
/// script.
trait Source {
    /// Replaces `into` with the reports made since the last call and returns
    /// `true`, or returns `false`, leaving `into` alone, if no frame was
    /// reported since.
    fn take(&mut self, into: &mut Vec<Report>) -> bool;
}

/// A pad without the standard mapping, which [`WebGamepads::poll`] skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unmapped {
    /// `Gamepad.index`.
    pub index: u32,
    /// `Gamepad.id`.
    pub id: String,
}

impl std::fmt::Display for Unmapped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pad {} ({:?}) has no standard mapping in this browser; ignoring it",
            self.index, self.id
        )
    }
}

/// What an index holds.
#[derive(Clone, Copy, Debug)]
enum Held {
    /// A standard-mapped pad: its id, family, and last reported snapshot.
    Mapped {
        pad: GamepadId,
        kind: PadKind,
        last: GamepadSnapshot,
    },
    /// A pad without the standard mapping, already returned as [`Unmapped`].
    Unmapped,
}

/// One index of `navigator.getGamepads()` that has a pad.
#[derive(Debug)]
struct Slot {
    index: u32,
    id: Vec<u8>,
    standard: bool,
    held: Held,
}

impl Slot {
    /// Whether `report` is still this slot's pad.
    fn is(&self, report: &Report) -> bool {
        report.index == self.index && report.standard == self.standard && report.id() == self.id
    }
}

/// The occupied indices, and the report buffer kept between polls.
#[derive(Debug, Default)]
struct Poller {
    /// Sorted by index.
    slots: Vec<Slot>,
    reports: Vec<Report>,
}

impl Poller {
    /// Takes a frame from `source` and emits what changed — see the module
    /// docs — returning the pads newly skipped for want of a mapping.
    fn poll(
        &mut self,
        source: &mut impl Source,
        emit: &mut impl FnMut(GamepadEvent),
    ) -> Vec<Unmapped> {
        let mut unmapped = Vec::new();
        if !source.take(&mut self.reports) {
            return unmapped;
        }
        self.reports.retain(|report| report.connected);
        self.reports.sort_by_key(|report| report.index);
        self.reports.dedup_by_key(|report| report.index);

        let reports = &self.reports;
        self.slots.retain(|slot| {
            let stays = reports.iter().any(|report| slot.is(report));
            if let (false, Held::Mapped { pad, .. }) = (stays, slot.held) {
                emit(GamepadEvent::Disconnected { id: pad });
            }
            stays
        });

        for report in reports {
            let at = self.slots.partition_point(|slot| slot.index < report.index);
            if self
                .slots
                .get(at)
                .is_none_or(|slot| slot.index != report.index)
            {
                let held = if report.standard {
                    let pad = GamepadId::allocate();
                    let kind = map::kind_of_id(&report.name());
                    emit(GamepadEvent::Connected { id: pad, kind });
                    Held::Mapped {
                        pad,
                        kind,
                        last: GamepadSnapshot::neutral(kind),
                    }
                } else {
                    unmapped.push(Unmapped {
                        index: report.index,
                        id: report.name(),
                    });
                    Held::Unmapped
                };
                let slot = Slot {
                    index: report.index,
                    id: report.id().to_vec(),
                    standard: report.standard,
                    held,
                };
                self.slots.insert(at, slot);
            }
            if let Held::Mapped { pad, kind, last } = &mut self.slots[at].held {
                let snapshot = map::snapshot_of(report.pressed, &report.values, *kind);
                if snapshot != *last {
                    *last = snapshot;
                    emit(GamepadEvent::State { id: *pad, snapshot });
                }
            }
        }
        unmapped
    }
}

/// The Web Gamepad API, as the page's shim reports it, with every index's
/// connection state.
///
/// One per page: every instance takes from the same frame of reports, and
/// the first to poll after the shim's pump gets it.
#[derive(Debug, Default)]
pub struct WebGamepads {
    poller: Poller,
}

impl WebGamepads {
    /// Nothing to open: the first poll reads whatever the shim has reported.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the frame the shim reported since the last call and calls `emit`
    /// with what changed: connections, disconnections, and snapshots that
    /// differ from the last one reported. A frame with no report changes
    /// nothing.
    ///
    /// Returns every pad this frame found without the standard mapping for
    /// the first time — skipped, not connected, and returned once each (see
    /// the module docs). Usually empty.
    #[must_use = "an unmapped pad is otherwise ignored without a word"]
    pub fn poll(&mut self, mut emit: impl FnMut(GamepadEvent)) -> Vec<Unmapped> {
        self.poller.poll(&mut shim::Bridge, &mut emit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionKind, ActionMap, Binding, PadAxis, PadButton};
    use map::tests::AT_REST;

    /// A connected, standard-mapped report at rest.
    fn pad(index: u32, id: &str) -> Report {
        let mut bytes = [0; ID_CAPACITY];
        bytes[..id.len()].copy_from_slice(id.as_bytes());
        Report {
            index,
            connected: true,
            standard: true,
            pressed: 0,
            values: AT_REST,
            id: bytes,
            id_len: id.len(),
        }
    }

    const XBOX: &str = "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)";
    const SONY: &str = "054c-0ce6-DualSense Wireless Controller";

    /// A script of frames: `None` is a frame the shim reported nothing for.
    #[derive(Debug, Default)]
    struct Fake {
        frame: Option<Vec<Report>>,
    }

    impl Source for Fake {
        fn take(&mut self, into: &mut Vec<Report>) -> bool {
            match self.frame.take() {
                Some(frame) => {
                    *into = frame;
                    true
                }
                None => false,
            }
        }
    }

    /// Polls one frame of `reports` (or none), returning the events and the
    /// pads skipped.
    fn poll(
        poller: &mut Poller,
        reports: Option<Vec<Report>>,
    ) -> (Vec<GamepadEvent>, Vec<Unmapped>) {
        let mut fake = Fake { frame: reports };
        let mut events = Vec::new();
        let unmapped = poller.poll(&mut fake, &mut |event| events.push(event));
        (events, unmapped)
    }

    /// **Connect, change, idle, disconnect, reconnect**, each reported once
    /// and only when it happens, with a fresh id for the pad that comes back.
    #[test]
    fn transitions_are_reported_once_each() {
        let mut poller = Poller::default();
        assert_eq!(poll(&mut poller, Some(vec![])), (vec![], vec![]));

        let (events, unmapped) = poll(&mut poller, Some(vec![pad(0, XBOX)]));
        assert!(unmapped.is_empty());
        let [GamepadEvent::Connected { id, kind }] = events[..] else {
            panic!("a pad at rest connects with no state: {events:?}");
        };
        assert_eq!(kind, PadKind::Xbox);

        let mut pressed = pad(0, XBOX);
        pressed.pressed = 1; // buttons[0]
        pressed.values[STANDARD_BUTTONS + 1] = -1.0; // axes[1], up
        let (events, _) = poll(&mut poller, Some(vec![pressed]));
        let [GamepadEvent::State { id: same, snapshot }] = events[..] else {
            panic!("one change, one state: {events:?}");
        };
        assert_eq!(same, id);
        assert!(snapshot.buttons.contains(PadButton::South));
        assert_eq!(snapshot.axis(PadAxis::LeftY), 1.0, "pushed up");

        let (events, _) = poll(&mut poller, Some(vec![pressed]));
        assert!(events.is_empty(), "the same snapshot again: {events:?}");
        let (events, _) = poll(&mut poller, None);
        assert!(events.is_empty(), "a frame with no report: {events:?}");

        let (events, _) = poll(&mut poller, Some(vec![pad(0, XBOX)]));
        assert_eq!(
            events,
            [GamepadEvent::State {
                id,
                snapshot: GamepadSnapshot::neutral(PadKind::Xbox),
            }]
        );

        let (events, _) = poll(&mut poller, Some(vec![]));
        assert_eq!(events, [GamepadEvent::Disconnected { id }]);

        let (events, _) = poll(&mut poller, Some(vec![pad(0, XBOX)]));
        let [GamepadEvent::Connected { id: again, .. }] = events[..] else {
            panic!("{events:?}");
        };
        assert_ne!(again, id, "a pad that comes back is a new pad");
    }

    /// A pad not at rest when it connects says so at once.
    #[test]
    fn a_pad_connected_mid_press_reports_its_state() {
        let mut poller = Poller::default();
        let mut held = pad(2, SONY);
        held.pressed = 1 << 1;
        held.values[7] = 1.0;
        let (events, _) = poll(&mut poller, Some(vec![held]));
        let [
            GamepadEvent::Connected { id, kind },
            GamepadEvent::State { id: same, snapshot },
        ] = events[..]
        else {
            panic!("{events:?}");
        };
        assert_eq!(kind, PadKind::PlayStation);
        assert_eq!(same, id);
        assert!(snapshot.buttons.contains(PadButton::East));
        assert_eq!(snapshot.axis(PadAxis::RightTrigger), 1.0);
    }

    /// **`connected: false` is a disconnect**, as much as the entry vanishing.
    #[test]
    fn a_pad_reporting_disconnected_is_gone() {
        let mut poller = Poller::default();
        let (events, _) = poll(&mut poller, Some(vec![pad(0, XBOX)]));
        let [GamepadEvent::Connected { id, .. }] = events[..] else {
            panic!("{events:?}");
        };
        let mut gone = pad(0, XBOX);
        gone.connected = false;
        assert_eq!(
            poll(&mut poller, Some(vec![gone])).0,
            [GamepadEvent::Disconnected { id }]
        );
        assert!(poll(&mut poller, Some(vec![gone])).0.is_empty());
    }

    /// **A different pad at a reused index** is the old one leaving and a new
    /// one arriving; the pad at another index is untouched.
    #[test]
    fn a_new_id_at_the_same_index_is_a_new_pad() {
        let mut poller = Poller::default();
        let (events, _) = poll(&mut poller, Some(vec![pad(0, XBOX), pad(1, XBOX)]));
        let [
            GamepadEvent::Connected { id: first, .. },
            GamepadEvent::Connected { id: second, .. },
        ] = events[..]
        else {
            panic!("{events:?}");
        };
        let (events, _) = poll(&mut poller, Some(vec![pad(0, SONY), pad(1, XBOX)]));
        let [
            GamepadEvent::Disconnected { id: gone },
            GamepadEvent::Connected { id: new, kind },
        ] = events[..]
        else {
            panic!("{events:?}");
        };
        assert_eq!(gone, first);
        assert_ne!(new, first);
        assert_ne!(new, second);
        assert_eq!(kind, PadKind::PlayStation);
    }

    /// **A pad without the standard mapping connects nothing** and is
    /// returned once, then again only after it has left and come back.
    #[test]
    fn an_unmapped_pad_is_skipped_and_returned_once() {
        let mut poller = Poller::default();
        let mut odd = pad(1, "Some Arcade Stick");
        odd.standard = false;
        odd.pressed = 1;
        let (events, unmapped) = poll(&mut poller, Some(vec![odd]));
        assert!(events.is_empty(), "{events:?}");
        assert_eq!(
            unmapped,
            [Unmapped {
                index: 1,
                id: "Some Arcade Stick".to_owned(),
            }]
        );
        assert_eq!(
            unmapped[0].to_string(),
            "pad 1 (\"Some Arcade Stick\") has no standard mapping in this browser; ignoring it"
        );
        assert_eq!(poll(&mut poller, Some(vec![odd])), (vec![], vec![]));
        assert_eq!(poll(&mut poller, Some(vec![])), (vec![], vec![]));
        assert_eq!(poll(&mut poller, Some(vec![odd])).1.len(), 1, "back again");

        let mut remapped = odd;
        remapped.standard = true;
        let (events, unmapped) = poll(&mut poller, Some(vec![remapped]));
        assert!(unmapped.is_empty());
        assert!(
            matches!(
                events[..],
                [GamepadEvent::Connected { .. }, GamepadEvent::State { .. }]
            ),
            "the same pad, standard-mapped, is a pad: {events:?}"
        );
    }

    /// Reports arrive in any order and are handled by index; a duplicate
    /// index counts once.
    #[test]
    fn reports_are_taken_by_index() {
        let mut poller = Poller::default();
        let (events, _) = poll(
            &mut poller,
            Some(vec![pad(3, SONY), pad(1, XBOX), pad(3, SONY)]),
        );
        let [
            GamepadEvent::Connected {
                kind: PadKind::Xbox,
                ..
            },
            GamepadEvent::Connected {
                kind: PadKind::PlayStation,
                ..
            },
        ] = events[..]
        else {
            panic!("index 1 first, index 3 once: {events:?}");
        };
    }

    /// **The poller's events drive a binding**, and an unplug releases it.
    #[test]
    fn polled_events_drive_an_action_map() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "fire".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::PadButton(PadButton::Guide)],
        });
        let mut poller = Poller::default();
        let mut held = pad(0, XBOX);
        held.pressed = 1 << 16;
        let mut fake = Fake {
            frame: Some(vec![held]),
        };
        let unmapped = poller.poll(&mut fake, &mut |event| map.gamepad_event(&event));
        assert!(unmapped.is_empty());
        assert!(map.just_pressed("fire"), "buttons[16] is Guide");

        fake.frame = Some(vec![]);
        let unmapped = poller.poll(&mut fake, &mut |event| map.gamepad_event(&event));
        assert!(unmapped.is_empty());
        assert!(map.just_released("fire"), "unplugging releases");
    }

    /// **Through the real entry points**, as `web/engine/gamepad.js` calls
    /// them: the scratch buffers written through their addresses, a `begin`,
    /// one report, and a poll — and no `begin`, no change.
    #[test]
    fn the_entry_points_carry_a_pad_into_the_poll() {
        use shim::{
            __crcbl_web_pad, __crcbl_web_pad_begin, __crcbl_web_pad_id_capacity,
            __crcbl_web_pad_id_ptr, __crcbl_web_pad_values_capacity, __crcbl_web_pad_values_ptr,
            PAD_CONNECTED, PAD_STANDARD,
        };

        let mut pads = WebGamepads::new();
        let mut events = Vec::new();
        assert!(pads.poll(|event| events.push(event)).is_empty());
        assert!(events.is_empty(), "nothing reported yet");

        assert_eq!(__crcbl_web_pad_values_capacity() as usize, VALUES);
        assert!(__crcbl_web_pad_id_capacity() as usize >= SONY.len());
        let values_at = __crcbl_web_pad_values_ptr();
        let id_at = __crcbl_web_pad_id_ptr();
        assert_eq!(values_at, __crcbl_web_pad_values_ptr(), "the address stays");
        let mut values = AT_REST;
        values[6] = 0.5; // the left trigger, half pulled
        values[STANDARD_BUTTONS + 3] = 1.0; // axes[3], the right stick down
        // What `new Float32Array(memory.buffer, ptr, n).set(values)` and
        // `TextEncoder.encodeInto` do.
        // SAFETY: both addresses start thread-local buffers that live for the
        // whole thread and hold at least these many elements (checked above
        // against their capacities); nothing else reaches them during the
        // copies.
        unsafe {
            core::ptr::copy_nonoverlapping(values.as_ptr(), values_at, VALUES);
            core::ptr::copy_nonoverlapping(SONY.as_ptr(), id_at, SONY.len());
        }
        __crcbl_web_pad_begin();
        __crcbl_web_pad(0, PAD_CONNECTED | PAD_STANDARD, 1 << 9, SONY.len() as u32);

        assert!(pads.poll(|event| events.push(event)).is_empty());
        let [
            GamepadEvent::Connected { id, kind },
            GamepadEvent::State { id: same, snapshot },
        ] = events[..]
        else {
            panic!("{events:?}");
        };
        assert_eq!(kind, PadKind::PlayStation, "the id crossed");
        assert_eq!(same, id);
        assert!(snapshot.buttons.contains(PadButton::Start));
        assert_eq!(snapshot.axis(PadAxis::LeftTrigger), 0.5);
        assert_eq!(snapshot.axis(PadAxis::RightY), -1.0, "down is -Y");

        events.clear();
        assert!(pads.poll(|event| events.push(event)).is_empty());
        assert!(events.is_empty(), "no begin since: the pad stays");

        __crcbl_web_pad_begin();
        assert!(pads.poll(|event| events.push(event)).is_empty());
        assert_eq!(events, [GamepadEvent::Disconnected { id }], "a begin alone");
    }
}
