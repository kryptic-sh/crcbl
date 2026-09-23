//! The Windows gamepad backend: XInput, polled, onto the gamepad seam.
//!
//! [`XInput::load`] finds the library, and [`XInput::poll`] reads all
//! four user slots and hands what changed to a closure as [`GamepadEvent`]s.
//! What the closure does with them is the game's: read them itself, hand them
//! to [`ActionMap::gamepad_event`](crate::ActionMap::gamepad_event), or both
//! (the seam's docs show the two side by side):
//!
//! ```no_run
//! # #[cfg(windows)]
//! # fn main() -> Result<(), crcbl_input::xinput::XInputError> {
//! use crcbl_input::{GamepadEvent, Stick};
//!
//! let mut pads = crcbl_input::xinput::XInput::load()?;
//! // Once a frame, before the frame's input is read:
//! pads.poll(|event| {
//!     if let GamepadEvent::State { id, snapshot } = event {
//!         let (x, y) = snapshot.stick(Stick::Left); // raw, +Y up, no dead zone
//!         println!("pad {} left stick at ({x}, {y})", id.0);
//!     }
//! })?;
//! # Ok(())
//! # }
//! # #[cfg(not(windows))]
//! # fn main() {}
//! ```
//!
//! # What it reports
//!
//! - **Connected** when a slot starts answering, with a fresh
//!   [`GamepadId`] — XInput names a slot, not a controller, so a pad unplugged
//!   and plugged back in is a new id — and [`PadKind::Xbox`], since XInput
//!   reports every controller in the Xbox layout.
//! - **State** when a slot's mapped snapshot differs from the last one it
//!   reported, and not on every poll. The map holds a snapshot as a level
//!   until the next arrives, so an identical one carries nothing, and after
//!   [`ActionMap::release_gamepads`](crate::ActionMap::release_gamepads) a pad
//!   left untouched stays neutral rather than being re-asserted by the next
//!   poll. The event stream is then proportional to what the player does,
//!   which is also what a recorder of it wants.
//! - **Disconnected** when a slot that was answering stops.
//!
//! # Empty slots are re-probed at an interval
//!
//! Microsoft's `XInputGetState` docs advise against calling it for an empty
//! slot every frame and suggest spacing out checks for new controllers. A slot
//! that answered "not connected" is therefore asked again only once
//! [`REPROBE_INTERVAL`] has passed, while a connected slot is read on every
//! poll. A pad plugged in is reported at most that interval after it arrives.
//!
//! # The mapping
//!
//! Positional buttons (A is [`PadButton::South`]), sticks normalised from
//! `i16` by one scale factor for both halves with the extra negative code
//! clamped (see [`stick_axis`]), triggers from `u8` to 0…1, and **no dead
//! zone**: the seam's bindings own it. XInput's Y is already +up. The Guide
//! button is never reported: `XInputGetState` does not expose it.
//!
//! # Not on other targets
//!
//! This module is compiled on Windows (and into every target's tests, which
//! is how the mapping is checked on Linux CI). No other target has a pad
//! backend yet, and none gets a stand-in that answers "no pads": a game that
//! names `xinput` elsewhere fails to build, rather than running as though it
//! had looked and found nothing.

mod ffi;

use crate::{GamepadEvent, GamepadId, GamepadSnapshot, PadAxis, PadButton, PadKind};
use ffi::{ERROR_DEVICE_NOT_CONNECTED, XInputGamepad, XInputState, XUSER_MAX_COUNT};
use std::time::{Duration, Instant};

/// What went wrong reaching XInput.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XInputError {
    /// Neither `xinput1_4.dll` nor `xinput9_1_0.dll` loaded with an
    /// `XInputGetState` in it. `code` is the last `GetLastError`.
    Unavailable {
        /// The Win32 error code.
        code: u32,
    },
    /// `XInputGetState` answered a slot with something other than success or
    /// "not connected". The slot is reported as disconnected, and every other
    /// slot was still polled.
    GetState {
        /// The user slot, 0…3.
        user: u32,
        /// The Win32 error code it returned.
        code: u32,
    },
}

impl std::fmt::Display for XInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable { code } => {
                write!(f, "no XInput library with XInputGetState (error {code})")
            }
            Self::GetState { user, code } => {
                write!(f, "XInputGetState failed for user {user} (error {code})")
            }
        }
    }
}

impl std::error::Error for XInputError {}

/// A stick axis from XInput's `i16` to −1…1.
///
/// **One scale for both halves**, 32767, so a push of *n* codes reads the same
/// distance either way and a circle stays a circle; the one code the negative
/// half has spare, −32768, clamps to −1 with −32767. Dividing each half by its
/// own extent would reach both ends too, at the price of the two halves
/// disagreeing about what a code is worth.
#[must_use]
pub fn stick_axis(value: i16) -> f32 {
    (f32::from(value) / f32::from(i16::MAX)).max(-1.0)
}

/// A trigger from XInput's `u8` to 0…1.
#[must_use]
pub fn trigger_axis(value: u8) -> f32 {
    f32::from(value) / f32::from(u8::MAX)
}

/// Each XInput button bit and the position it is.
const BUTTONS: [(u16, PadButton); 14] = [
    (ffi::XINPUT_GAMEPAD_A, PadButton::South),
    (ffi::XINPUT_GAMEPAD_B, PadButton::East),
    (ffi::XINPUT_GAMEPAD_X, PadButton::West),
    (ffi::XINPUT_GAMEPAD_Y, PadButton::North),
    (ffi::XINPUT_GAMEPAD_LEFT_SHOULDER, PadButton::LeftShoulder),
    (ffi::XINPUT_GAMEPAD_RIGHT_SHOULDER, PadButton::RightShoulder),
    (ffi::XINPUT_GAMEPAD_LEFT_THUMB, PadButton::LeftStick),
    (ffi::XINPUT_GAMEPAD_RIGHT_THUMB, PadButton::RightStick),
    (ffi::XINPUT_GAMEPAD_START, PadButton::Start),
    (ffi::XINPUT_GAMEPAD_BACK, PadButton::Select),
    (ffi::XINPUT_GAMEPAD_DPAD_UP, PadButton::DpadUp),
    (ffi::XINPUT_GAMEPAD_DPAD_DOWN, PadButton::DpadDown),
    (ffi::XINPUT_GAMEPAD_DPAD_LEFT, PadButton::DpadLeft),
    (ffi::XINPUT_GAMEPAD_DPAD_RIGHT, PadButton::DpadRight),
];

/// One XInput pad state as the seam's snapshot.
fn snapshot_of(pad: &XInputGamepad) -> GamepadSnapshot {
    let mut snapshot = GamepadSnapshot::neutral(PadKind::Xbox);
    snapshot.buttons = BUTTONS
        .iter()
        .filter(|(bit, _)| pad.buttons & bit != 0)
        .map(|&(_, button)| button)
        .collect();
    snapshot.axes[PadAxis::LeftX as usize] = stick_axis(pad.thumb_lx);
    snapshot.axes[PadAxis::LeftY as usize] = stick_axis(pad.thumb_ly);
    snapshot.axes[PadAxis::RightX as usize] = stick_axis(pad.thumb_rx);
    snapshot.axes[PadAxis::RightY as usize] = stick_axis(pad.thumb_ry);
    snapshot.axes[PadAxis::LeftTrigger as usize] = trigger_axis(pad.left_trigger);
    snapshot.axes[PadAxis::RightTrigger as usize] = trigger_axis(pad.right_trigger);
    snapshot
}

/// Where the poller reads a slot from: the real `XInputGetState`, or a test's
/// script.
trait StateSource {
    /// The slot's state, or the Win32 error code `XInputGetState` returned.
    fn get_state(&mut self, user: u32) -> Result<XInputState, u32>;
}

/// A connected slot: its id, and the snapshot last reported for it.
#[derive(Clone, Copy, Debug)]
struct Slot {
    id: GamepadId,
    last: GamepadSnapshot,
}

/// How long a slot that answered "not connected" goes unasked.
///
/// A second: a pad takes a moment to plug in and enumerate anyway, so a
/// connection reported that late is not something a player notices, while the
/// empty-slot probe stops being a cost paid on every frame.
pub const REPROBE_INTERVAL: Duration = Duration::from_secs(1);

/// The four slots' connection state, and the transitions a poll reports.
#[derive(Debug, Default)]
struct Poller {
    slots: [Option<Slot>; XUSER_MAX_COUNT as usize],
    /// When each slot last answered "not connected". A slot is read only once
    /// that is an interval old, so the stale time a slot keeps after it
    /// connects never skips a read.
    found_empty: [Option<Instant>; XUSER_MAX_COUNT as usize],
}

impl Poller {
    /// Reads every slot due a read at `now` and emits what changed — see the
    /// module docs. `now` is the caller's, so a test can step it.
    fn poll(
        &mut self,
        source: &mut impl StateSource,
        now: Instant,
        emit: &mut impl FnMut(GamepadEvent),
    ) -> Result<(), XInputError> {
        let mut failure = None;
        let slots = self.slots.iter_mut().zip(&mut self.found_empty);
        for (user, (slot, found_empty)) in (0..XUSER_MAX_COUNT).zip(slots) {
            if found_empty.is_some_and(|at| now.duration_since(at) < REPROBE_INTERVAL) {
                continue;
            }
            match source.get_state(user) {
                Ok(state) => {
                    let snapshot = snapshot_of(&state.gamepad);
                    let connected = slot.get_or_insert_with(|| {
                        let id = GamepadId::allocate();
                        emit(GamepadEvent::Connected {
                            id,
                            kind: PadKind::Xbox,
                        });
                        Slot {
                            id,
                            last: GamepadSnapshot::neutral(PadKind::Xbox),
                        }
                    });
                    if connected.last != snapshot {
                        connected.last = snapshot;
                        emit(GamepadEvent::State {
                            id: connected.id,
                            snapshot,
                        });
                    }
                }
                Err(code) => {
                    if let Some(gone) = slot.take() {
                        emit(GamepadEvent::Disconnected { id: gone.id });
                    }
                    if code == ERROR_DEVICE_NOT_CONNECTED {
                        *found_empty = Some(now);
                    } else {
                        failure = Some(XInputError::GetState { user, code });
                    }
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

#[cfg(windows)]
pub use loaded::XInput;

#[cfg(windows)]
mod loaded {
    use super::ffi::{
        ERROR_SUCCESS, FreeLibrary, GetLastError, GetProcAddress, LoadLibraryW, Module,
        XInputGetStateFn, XInputState,
    };
    use super::{Poller, StateSource, XInputError};
    use crate::GamepadEvent;
    use std::time::Instant;

    /// The libraries tried, in order.
    const LIBRARIES: [&str; 2] = ["xinput1_4.dll", "xinput9_1_0.dll"];

    /// A loaded XInput library and its `XInputGetState`.
    #[derive(Debug)]
    pub(super) struct Library {
        module: Module,
        get_state: XInputGetStateFn,
        name: &'static str,
    }

    impl Library {
        pub(super) fn load() -> Result<Self, XInputError> {
            let mut code = 0;
            for name in LIBRARIES {
                let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
                // SAFETY: `wide` is a NUL-terminated UTF-16 string that lives
                // across the call.
                let module = unsafe { LoadLibraryW(wide.as_ptr()) };
                if module.is_null() {
                    // SAFETY: no arguments; reads this thread's last error.
                    code = unsafe { GetLastError() };
                    continue;
                }
                // SAFETY: `module` is the handle just loaded, and the name is
                // a NUL-terminated ANSI string.
                let proc = unsafe { GetProcAddress(module, c"XInputGetState".as_ptr().cast()) };
                if proc.is_null() {
                    // SAFETY: as above; then the module is released, once.
                    unsafe {
                        code = GetLastError();
                        FreeLibrary(module);
                    }
                    continue;
                }
                // SAFETY: `XInputGetState` is exported with exactly this
                // signature by both libraries (`Xinput.h`), and `module`
                // stays loaded for as long as the pointer is kept — `Drop`
                // frees it only with the pointer.
                let get_state = unsafe {
                    core::mem::transmute::<*mut core::ffi::c_void, XInputGetStateFn>(proc)
                };
                return Ok(Self {
                    module,
                    get_state,
                    name,
                });
            }
            Err(XInputError::Unavailable { code })
        }
    }

    impl StateSource for Library {
        fn get_state(&mut self, user: u32) -> Result<XInputState, u32> {
            let mut state = XInputState::default();
            // SAFETY: `get_state` is live while `self` is (see `load`), and
            // `state` is a writable `XINPUT_STATE` for the call's duration.
            let code = unsafe { (self.get_state)(user, &raw mut state) };
            if code == ERROR_SUCCESS {
                Ok(state)
            } else {
                Err(code)
            }
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            // SAFETY: the module was loaded once in `load` and is freed once,
            // here, together with the only copy of the pointer into it.
            let freed = unsafe { FreeLibrary(self.module) };
            // Asserted rather than returned: a drop has no caller to tell, and
            // a failure here means the handle was never ours.
            debug_assert_ne!(freed, 0, "FreeLibrary refused {}", self.name);
        }
    }

    /// XInput, loaded, with the four slots' connection state.
    ///
    /// Not `Send`: it holds the library's module handle. Poll it from the
    /// thread that owns the frame.
    #[derive(Debug)]
    pub struct XInput {
        library: Library,
        poller: Poller,
    }

    impl XInput {
        /// Loads `xinput1_4.dll`, or `xinput9_1_0.dll` where that is missing.
        ///
        /// # Errors
        /// [`XInputError::Unavailable`] if neither loads with an
        /// `XInputGetState` in it.
        pub fn load() -> Result<Self, XInputError> {
            Ok(Self {
                library: Library::load()?,
                poller: Poller::default(),
            })
        }

        /// The library that loaded.
        #[must_use]
        pub fn library_name(&self) -> &'static str {
            self.library.name
        }

        /// Reads the four slots and calls `emit` with what changed:
        /// connections, disconnections, and snapshots that differ from the
        /// last one reported. A connected slot is read on every call; an
        /// empty one only once [`REPROBE_INTERVAL`](super::REPROBE_INTERVAL)
        /// has passed since it was last found empty.
        ///
        /// # Errors
        /// [`XInputError::GetState`] if a slot answered with an unexpected
        /// error; the other slots were polled regardless, and that one is
        /// reported as disconnected.
        pub fn poll(&mut self, mut emit: impl FnMut(GamepadEvent)) -> Result<(), XInputError> {
            self.poller
                .poll(&mut self.library, Instant::now(), &mut emit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionKind, ActionMap, Binding, PadButtons};

    /// Hand-worked values at both ends, the centre, and the one code the
    /// negative half has spare.
    #[test]
    fn stick_and_trigger_values_normalise_against_known_codes() {
        assert_eq!(stick_axis(-32768), -1.0, "the spare code clamps");
        assert_eq!(stick_axis(-32767), -1.0);
        assert_eq!(stick_axis(0), 0.0);
        assert_eq!(stick_axis(32767), 1.0);
        assert_eq!(
            stick_axis(-16384),
            -stick_axis(16384),
            "one scale both ways"
        );
        assert_eq!(trigger_axis(0), 0.0);
        assert_eq!(trigger_axis(255), 1.0);
        assert!((trigger_axis(128) - 128.0 / 255.0).abs() < f32::EPSILON);
    }

    /// Every button bit lands on its position, and +Y stays up.
    #[test]
    fn the_raw_state_maps_positionally_with_y_up() {
        for &(bit, button) in &BUTTONS {
            let snapshot = snapshot_of(&XInputGamepad {
                buttons: bit,
                ..XInputGamepad::default()
            });
            assert_eq!(
                snapshot.buttons,
                [button].into_iter().collect::<PadButtons>(),
                "bit {bit:#06x}"
            );
        }
        let snapshot = snapshot_of(&XInputGamepad {
            thumb_ly: i16::MAX,
            thumb_rx: i16::MIN,
            right_trigger: u8::MAX,
            ..XInputGamepad::default()
        });
        assert_eq!(snapshot.axis(PadAxis::LeftY), 1.0, "pushed up is +Y");
        assert_eq!(snapshot.axis(PadAxis::RightX), -1.0);
        assert_eq!(snapshot.axis(PadAxis::RightTrigger), 1.0);
        assert_eq!(snapshot.kind, PadKind::Xbox);
    }

    /// Four slots answered from a script, counting the reads of each, with
    /// the clock the tests poll at.
    #[derive(Debug)]
    struct Fake {
        slots: [Result<XInputState, u32>; 4],
        reads: [u32; 4],
        now: Instant,
    }

    impl Fake {
        fn new(slots: [Result<XInputState, u32>; 4]) -> Self {
            Self {
                slots,
                reads: [0; 4],
                now: Instant::now(),
            }
        }
    }

    impl StateSource for Fake {
        fn get_state(&mut self, user: u32) -> Result<XInputState, u32> {
            self.reads[user as usize] += 1;
            self.slots[user as usize]
        }
    }

    const EMPTY: Result<XInputState, u32> = Err(ERROR_DEVICE_NOT_CONNECTED);

    fn pad(buttons: u16) -> Result<XInputState, u32> {
        Ok(XInputState {
            packet_number: 0,
            gamepad: XInputGamepad {
                buttons,
                ..XInputGamepad::default()
            },
        })
    }

    /// Polls `after` the fake's clock, moving the clock there.
    fn poll_after(
        poller: &mut Poller,
        fake: &mut Fake,
        after: Duration,
    ) -> (Vec<GamepadEvent>, Result<(), XInputError>) {
        fake.now += after;
        let now = fake.now;
        let mut events = Vec::new();
        let result = poller.poll(fake, now, &mut |event| events.push(event));
        (events, result)
    }

    /// Polls a whole re-probe interval after the last poll, so every slot is
    /// read.
    fn poll(poller: &mut Poller, fake: &mut Fake) -> (Vec<GamepadEvent>, Result<(), XInputError>) {
        poll_after(poller, fake, REPROBE_INTERVAL)
    }

    /// **An empty slot is not asked again within the interval**, and is once
    /// it has passed.
    #[test]
    fn an_empty_slot_is_reprobed_only_after_the_interval() {
        let mut poller = Poller::default();
        let mut fake = Fake::new([EMPTY; 4]);
        poll(&mut poller, &mut fake).1.expect("scripted");
        assert_eq!(fake.reads, [1; 4], "the first poll asks every slot");

        let almost = REPROBE_INTERVAL - Duration::from_millis(1);
        poll_after(&mut poller, &mut fake, Duration::ZERO)
            .1
            .expect("scripted");
        poll_after(&mut poller, &mut fake, almost)
            .1
            .expect("scripted");
        assert_eq!(fake.reads, [1; 4], "nothing re-asked inside the interval");

        poll_after(&mut poller, &mut fake, Duration::from_millis(1))
            .1
            .expect("scripted");
        assert_eq!(fake.reads, [2; 4], "every slot re-asked once it passed");
    }

    /// **A connected slot is read on every poll**, however close together,
    /// while the empty ones beside it wait out the interval.
    #[test]
    fn a_connected_slot_is_read_every_poll() {
        let mut poller = Poller::default();
        let mut fake = Fake::new([EMPTY, pad(0), EMPTY, EMPTY]);
        poll(&mut poller, &mut fake).1.expect("scripted");
        for _ in 0..3 {
            poll_after(&mut poller, &mut fake, Duration::ZERO)
                .1
                .expect("scripted");
        }
        assert_eq!(fake.reads, [1, 4, 1, 1]);

        fake.slots[1] = pad(ffi::XINPUT_GAMEPAD_A);
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        assert!(
            matches!(events[..], [GamepadEvent::State { .. }]),
            "a press on the connected pad arrives at once: {events:?}"
        );
    }

    /// **A pad plugged into an empty slot is reported within one interval**
    /// of the slot last being found empty, and not before it.
    #[test]
    fn a_new_pad_is_detected_within_one_interval() {
        let mut poller = Poller::default();
        let mut fake = Fake::new([EMPTY; 4]);
        poll(&mut poller, &mut fake).1.expect("scripted");

        let half = REPROBE_INTERVAL / 2;
        let (events, _) = poll_after(&mut poller, &mut fake, half);
        assert!(events.is_empty(), "{events:?}");
        fake.slots[2] = pad(0);
        let (events, _) = poll_after(&mut poller, &mut fake, half - Duration::from_millis(1));
        assert!(events.is_empty(), "still inside the interval: {events:?}");

        let (events, _) = poll_after(&mut poller, &mut fake, Duration::from_millis(1));
        assert!(
            matches!(events[..], [GamepadEvent::Connected { .. }]),
            "{events:?}"
        );
    }

    /// A slot that disconnects is empty like any other, so a pad plugged back
    /// in is found once the interval has passed.
    #[test]
    fn an_unplugged_slot_is_reprobed_after_the_interval() {
        let mut poller = Poller::default();
        let mut fake = Fake::new([pad(0), EMPTY, EMPTY, EMPTY]);
        poll(&mut poller, &mut fake).1.expect("scripted");
        fake.slots[0] = EMPTY;
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        assert!(matches!(events[..], [GamepadEvent::Disconnected { .. }]));

        fake.slots[0] = pad(0);
        let (events, _) = poll_after(&mut poller, &mut fake, Duration::ZERO);
        assert!(events.is_empty(), "inside the interval: {events:?}");
        let (events, _) = poll(&mut poller, &mut fake);
        assert!(matches!(events[..], [GamepadEvent::Connected { .. }]));
    }

    /// **Connect, change, disconnect, reconnect**, each reported once and only
    /// when it happens, with a fresh id for a pad that comes back.
    #[test]
    fn slot_transitions_are_reported_once_each() {
        let mut poller = Poller::default();
        let mut fake = Fake::new([EMPTY; 4]);
        assert_eq!(poll(&mut poller, &mut fake), (vec![], Ok(())), "all empty");

        fake.slots[1] = pad(ffi::XINPUT_GAMEPAD_A);
        let (events, result) = poll(&mut poller, &mut fake);
        assert_eq!(result, Ok(()));
        let [
            GamepadEvent::Connected { id, kind },
            GamepadEvent::State { id: same, snapshot },
        ] = events[..]
        else {
            panic!("a connection and its first state: {events:?}");
        };
        assert_eq!((kind, same), (PadKind::Xbox, id));
        assert!(snapshot.buttons.contains(PadButton::South));

        assert_eq!(poll(&mut poller, &mut fake), (vec![], Ok(())), "unchanged");

        fake.slots[1] = pad(0);
        let (events, _) = poll(&mut poller, &mut fake);
        assert_eq!(
            events,
            [GamepadEvent::State {
                id,
                snapshot: GamepadSnapshot::neutral(PadKind::Xbox),
            }],
        );

        fake.slots[1] = EMPTY;
        assert_eq!(
            poll(&mut poller, &mut fake),
            (vec![GamepadEvent::Disconnected { id }], Ok(())),
        );

        fake.slots[1] = pad(0);
        let (events, _) = poll(&mut poller, &mut fake);
        let [GamepadEvent::Connected { id: again, .. }] = events[..] else {
            panic!("a reconnection at rest is one event: {events:?}");
        };
        assert_ne!(again, id, "XInput names a slot, not a pad");
    }

    /// An unexpected error disconnects that slot, is returned, and does not
    /// stop the other slots being polled.
    #[test]
    fn an_unexpected_error_disconnects_the_slot_and_is_reported() {
        let mut poller = Poller::default();
        let mut fake = Fake::new([pad(0), EMPTY, EMPTY, EMPTY]);
        let (events, _) = poll(&mut poller, &mut fake);
        let [GamepadEvent::Connected { id, .. }] = events[..] else {
            panic!("{events:?}");
        };

        fake.slots[0] = Err(5);
        fake.slots[3] = pad(ffi::XINPUT_GAMEPAD_B);
        let (events, result) = poll(&mut poller, &mut fake);
        assert_eq!(result, Err(XInputError::GetState { user: 0, code: 5 }));
        assert_eq!(events[0], GamepadEvent::Disconnected { id });
        assert!(
            matches!(events[1], GamepadEvent::Connected { .. }),
            "slot 3 was still polled: {events:?}"
        );
    }

    /// **The events are usable with no `ActionMap`**: a game-owned adapter in
    /// the shape of EW's `gamepad_input` — a device id widened to `u64`, raw
    /// finite sticks with +Y up, its own dead zone, a disconnect that clears —
    /// consumes the poller's stream directly.
    #[test]
    fn polled_events_feed_a_game_adapter_with_no_action_map() {
        #[derive(Default)]
        struct Adapter {
            device: Option<u64>,
            left: (f32, f32),
        }
        impl Adapter {
            fn feed(&mut self, event: GamepadEvent) {
                match event {
                    GamepadEvent::State { id, snapshot } => {
                        assert!(snapshot.is_finite());
                        self.device = Some(u64::from(id.0));
                        self.left = snapshot.stick(crate::Stick::Left);
                    }
                    GamepadEvent::Disconnected { id } if self.device == Some(u64::from(id.0)) => {
                        *self = Self::default();
                    }
                    _ => {}
                }
            }
        }

        let mut adapter = Adapter::default();
        let mut poller = Poller::default();
        let mut fake = Fake::new([EMPTY; 4]);
        fake.slots[0] = Ok(XInputState {
            packet_number: 1,
            gamepad: XInputGamepad {
                // Resting drift: a binding's dead zone would zero it, and the
                // adapter must see it raw to apply its own.
                thumb_lx: 3277,
                thumb_ly: i16::MAX,
                ..XInputGamepad::default()
            },
        });
        poller
            .poll(&mut fake, Instant::now(), &mut |event| adapter.feed(event))
            .expect("scripted");
        assert_eq!(adapter.left, (stick_axis(3277), 1.0), "raw, and +Y up");
        let device = adapter.device.expect("a state arrived");

        fake.slots[0] = EMPTY;
        poller
            .poll(&mut fake, Instant::now(), &mut |event| adapter.feed(event))
            .expect("scripted");
        assert_eq!(adapter.device, None, "pad {device} unplugged");
        assert_eq!(adapter.left, (0.0, 0.0));
    }

    /// **The poller's events drive a binding** — the backend and the seam
    /// meeting, with no hardware.
    #[test]
    fn polled_events_drive_an_action_map() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "jump".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::PadButton(PadButton::South)],
        });
        let mut poller = Poller::default();
        let mut fake = Fake::new([EMPTY, EMPTY, pad(ffi::XINPUT_GAMEPAD_A), EMPTY]);
        poller
            .poll(&mut fake, Instant::now(), &mut |event| {
                map.gamepad_event(&event)
            })
            .expect("scripted");
        assert!(map.just_pressed("jump"));

        fake.slots[2] = EMPTY;
        poller
            .poll(&mut fake, Instant::now(), &mut |event| {
                map.gamepad_event(&event)
            })
            .expect("scripted");
        assert!(map.just_released("jump"), "unplugging releases");
    }

    /// **The real `XInputGetState`, on a host with no pad in some slot**,
    /// answers `ERROR_DEVICE_NOT_CONNECTED` there, and the poller takes that as
    /// an empty slot rather than an error.
    ///
    /// Fails on a host with a controller in all four slots, which is the one
    /// arrangement where there is no empty slot to ask about.
    #[cfg(windows)]
    #[test]
    fn the_real_library_answers_an_empty_slot_as_not_connected() {
        let mut library = loaded::Library::load().expect("XInput ships with Windows");
        let answers: Vec<Result<(), u32>> = (0..XUSER_MAX_COUNT)
            .map(|user| library.get_state(user).map(|_| ()))
            .collect();
        assert!(
            answers
                .iter()
                .all(|answer| matches!(answer, Ok(()) | Err(ERROR_DEVICE_NOT_CONNECTED))),
            "{answers:?}",
        );
        let empty = answers.iter().filter(|answer| answer.is_err()).count();
        assert!(empty > 0, "every slot has a pad, so none could answer 1167");

        let mut events = Vec::new();
        Poller::default()
            .poll(&mut library, Instant::now(), &mut |event| {
                events.push(event)
            })
            .expect("1167 is an empty slot, not an error");
        let connected = events
            .iter()
            .filter(|event| matches!(event, GamepadEvent::Connected { .. }))
            .count();
        assert_eq!(connected, answers.len() - empty);
    }
}
