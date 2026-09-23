//! The macOS gamepad backend: GameController.framework, polled, onto the
//! gamepad seam.
//!
//! [`GameController::new`] finds the `GCController` class, and
//! [`GameController::poll`] reads every controller the framework lists and
//! hands what changed to a closure as [`GamepadEvent`]s — the same shape as
//! `xinput`, `evdev` and `web_gamepad`, so a game consumes it the same way:
//!
//! ```no_run
//! # #[cfg(target_os = "macos")]
//! # fn main() -> Result<(), crcbl_input::game_controller::GameControllerError> {
//! use crcbl_input::{GamepadEvent, Stick};
//!
//! let mut pads = crcbl_input::game_controller::GameController::new()?;
//! // Once a frame, on the thread that pumps the AppKit event loop:
//! pads.poll(|event| {
//!     if let GamepadEvent::State { id, snapshot } = event {
//!         let (x, y) = snapshot.stick(Stick::Left); // raw, +Y up, no dead zone
//!         println!("pad {} left stick at ({x}, {y})", id.0);
//!     }
//! });
//! # Ok(())
//! # }
//! # #[cfg(not(target_os = "macos"))]
//! # fn main() {}
//! ```
//!
//! # Discovery needs a running event loop
//!
//! GameController finds controllers and updates their elements from the
//! process's main run loop: `[GCController controllers]` is only as current as
//! the last turn of that loop. The engine's AppKit shell turns it on every
//! `pump`, through `nextEventMatchingMask:untilDate:inMode:dequeue:`, so a
//! windowed run sees pads plugged in before or after it starts. **A process that
//! never turns the main run loop** — a headless run, a command-line tool, a test
//! binary — gets an empty array, or controllers whose values never move; the
//! call itself is safe and simply reports nothing. The engine gives a headless
//! run no pad source at all anyway (`crcbl::engine::pads`), so this matters
//! only to a caller that polls this module outside the engine.
//!
//! By default the framework also stops delivering input while another
//! application is frontmost (`GCController.shouldMonitorBackgroundEvents`,
//! which this backend leaves at its default); the engine releases every pad on
//! focus loss regardless.
//!
//! # What it reports
//!
//! - **Connected** when a controller with an `extendedGamepad` profile first
//!   appears in `[GCController controllers]`, with a fresh [`GamepadId`] and the
//!   [`PadKind`] its `productCategory`, or else its `vendorName`, names (see
//!   [`kind_of`]). Then a **State** if the pad is not at rest.
//! - **State** when a pad's mapped snapshot differs from the last one reported
//!   — change-only, as the other backends are, and for the same reasons.
//! - **Disconnected** when the controller leaves the list, or loses its
//!   extended profile.
//!
//! Controllers are told apart by **object identity**: the backend retains
//! every controller it reports for as long as it is listed, so a controller
//! that goes away cannot hand its address to the next one while the first is
//! still being tracked. A pad unplugged and plugged back in is a new
//! `GCController` object and so a new id. No `GCControllerDidConnect`
//! observer: the list is read every frame anyway, so the notification would
//! only say one frame early what the poll says.
//!
//! A controller with no `extendedGamepad` profile — a Siri Remote, a
//! `microGamepad`-only device — is not a pad here and is not connected at all.
//!
//! # The mapping
//!
//! GameController names the face buttons **by Xbox position whatever the
//! device prints on them** — `buttonA` is the bottom button on a DualSense
//! too — so the names map positionally with no per-family table:
//!
//! | `extendedGamepad`                              | seam                                   |
//! | ---------------------------------------------- | -------------------------------------- |
//! | `buttonA`, `buttonB`, `buttonX`, `buttonY`     | `South`, `East`, `West`, `North`       |
//! | `leftShoulder`, `rightShoulder`                | `LeftShoulder`, `RightShoulder`        |
//! | `leftThumbstickButton`, `rightThumbstickButton` | `LeftStick`, `RightStick`             |
//! | `buttonMenu`, `buttonOptions`, `buttonHome`    | `Start`, `Select`, `Guide`             |
//! | `dpad.up`, `.down`, `.left`, `.right`          | `DpadUp`, `DpadDown`, `DpadLeft`, `DpadRight` |
//! | `leftTrigger.value`, `rightTrigger.value`      | `LeftTrigger`, `RightTrigger`, 0…1     |
//! | `leftThumbstick.xAxis`/`.yAxis` `.value`       | the left stick, **Y not flipped**      |
//! | `rightThumbstick.xAxis`/`.yAxis` `.value`      | the right stick, likewise              |
//!
//! Buttons are read from `isPressed` and analog elements from `value`, clamped
//! by [`stick_axis`] and [`trigger_axis`]; **no dead zone** is applied, as the
//! seam requires. GameController's thumbstick Y is already +up — SDL's
//! GameController joystick driver negates it to reach its own +down
//! convention — so it passes through unchanged. Elements newer than the
//! running macOS (`buttonHome` is macOS 11, `buttonMenu` and `buttonOptions`
//! 10.15, the thumbstick buttons 10.14.1) or absent from the device are read as
//! released. macOS may keep the Home button for a system gesture, in which case
//! `Guide` is never reported.
//!
//! # Not on other targets
//!
//! This module is compiled on macOS (and into every target's tests, which is
//! how the mapping, the classification and the poller are checked on the
//! Windows and Linux runners); only the framework access is macOS-only. No
//! other target gets a stand-in that answers "no pads".

#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod macos;

pub use crate::float_axis::{stick_axis, trigger_axis};
use crate::product_name::kind_of_name;
use crate::{GamepadEvent, GamepadId, GamepadSnapshot, PadAxis, PadButton, PadKind};
use core::ffi::CStr;
#[cfg(target_os = "macos")]
pub use macos::GameController;

/// What went wrong reaching GameController.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameControllerError {
    /// The Objective-C runtime has no `GCController` class, which means
    /// GameController.framework is not in the process image.
    Unavailable,
}

impl std::fmt::Display for GameControllerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(f, "GameController.framework has no GCController class"),
        }
    }
}

impl std::error::Error for GameControllerError {}

/// A chain of property getters from an `extendedGamepad` profile to one
/// element: `[profile buttonA]`, or `[[profile dpad] up]`.
type Path = &'static [&'static CStr];

/// Every button element read, by its path from the profile, and the position
/// it is. The macOS half walks these same paths, so the table the tests check
/// is the table the framework is read by.
const BUTTONS: [(Path, PadButton); 15] = [
    (&[c"buttonA"], PadButton::South),
    (&[c"buttonB"], PadButton::East),
    (&[c"buttonX"], PadButton::West),
    (&[c"buttonY"], PadButton::North),
    (&[c"leftShoulder"], PadButton::LeftShoulder),
    (&[c"rightShoulder"], PadButton::RightShoulder),
    (&[c"leftThumbstickButton"], PadButton::LeftStick),
    (&[c"rightThumbstickButton"], PadButton::RightStick),
    (&[c"buttonMenu"], PadButton::Start),
    (&[c"buttonOptions"], PadButton::Select),
    (&[c"buttonHome"], PadButton::Guide),
    (&[c"dpad", c"up"], PadButton::DpadUp),
    (&[c"dpad", c"down"], PadButton::DpadDown),
    (&[c"dpad", c"left"], PadButton::DpadLeft),
    (&[c"dpad", c"right"], PadButton::DpadRight),
];

/// Every analog element read, by its path, and the axis it is. A trigger is a
/// button element whose `value` is its pull; a stick axis is an axis element.
const AXES: [(Path, PadAxis); 6] = [
    (&[c"leftThumbstick", c"xAxis"], PadAxis::LeftX),
    (&[c"leftThumbstick", c"yAxis"], PadAxis::LeftY),
    (&[c"rightThumbstick", c"xAxis"], PadAxis::RightX),
    (&[c"rightThumbstick", c"yAxis"], PadAxis::RightY),
    (&[c"leftTrigger"], PadAxis::LeftTrigger),
    (&[c"rightTrigger"], PadAxis::RightTrigger),
];

/// One controller's elements as read this poll, row for row with [`BUTTONS`]
/// and [`AXES`]: plain data, so the mapping runs with no framework.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Reading {
    /// `isPressed` of each [`BUTTONS`] row.
    pressed: [bool; BUTTONS.len()],
    /// `value` of each [`AXES`] row, as the framework reported it.
    values: [f32; AXES.len()],
}

/// One reading as the seam's snapshot — see the module docs' table.
fn snapshot_of(reading: &Reading, kind: PadKind) -> GamepadSnapshot {
    let mut snapshot = GamepadSnapshot::neutral(kind);
    snapshot.buttons = BUTTONS
        .iter()
        .zip(reading.pressed)
        .filter(|&(_, pressed)| pressed)
        .map(|(&(_, button), _)| button)
        .collect();
    for (&(_, axis), value) in AXES.iter().zip(reading.values) {
        snapshot.axes[axis as usize] = match axis {
            PadAxis::LeftTrigger | PadAxis::RightTrigger => trigger_axis(value),
            PadAxis::LeftX | PadAxis::LeftY | PadAxis::RightX | PadAxis::RightY => {
                stick_axis(value)
            }
        };
    }
    snapshot
}

/// A controller's family from its `productCategory` (macOS 10.15 and later),
/// or from its `vendorName` where the category names none.
///
/// Both go through the same name match the browser backend falls back on. The
/// category is tried first because it is a fixed vocabulary — `"DualSense"`,
/// `"Xbox One"`, `"Switch Pro Controller"` — while the vendor name is whatever
/// the device calls itself; `"MFi"` and `"HID"` categories name no family and
/// fall through to the name.
#[must_use]
pub fn kind_of(product_category: Option<&str>, vendor_name: Option<&str>) -> PadKind {
    [product_category, vendor_name]
        .into_iter()
        .flatten()
        .map(kind_of_name)
        .find(|&kind| kind != PadKind::Generic)
        .unwrap_or(PadKind::Generic)
}

/// A controller's identity: its object's address, which the source keeps
/// unique by retaining the object while it is listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key(usize);

/// One controller in this poll's list.
#[derive(Clone, Copy, Debug)]
struct Seen {
    key: Key,
    /// Its family, which the source reads once, when it first lists it.
    kind: PadKind,
    reading: Reading,
}

/// Where the poller reads the controllers from: the framework, or a test's
/// script.
trait Source {
    /// Replaces `into` with every controller listed now that has an
    /// extended gamepad profile, in the framework's order.
    fn read(&mut self, into: &mut Vec<Seen>);
}

/// A connected controller: its identity, id, family and last snapshot.
#[derive(Clone, Copy, Debug)]
struct Slot {
    key: Key,
    id: GamepadId,
    kind: PadKind,
    last: GamepadSnapshot,
}

/// The connected controllers, and the list buffer kept between polls.
#[derive(Debug, Default)]
struct Poller {
    slots: Vec<Slot>,
    seen: Vec<Seen>,
}

impl Poller {
    /// Reads the list and emits what changed — see the module docs:
    /// disconnections first, then connections and states in list order.
    fn poll(&mut self, source: &mut impl Source, emit: &mut impl FnMut(GamepadEvent)) {
        source.read(&mut self.seen);
        let seen = &self.seen;
        self.slots.retain(|slot| {
            let stays = seen.iter().any(|controller| controller.key == slot.key);
            if !stays {
                emit(GamepadEvent::Disconnected { id: slot.id });
            }
            stays
        });
        for controller in seen {
            let at = match self
                .slots
                .iter()
                .position(|slot| slot.key == controller.key)
            {
                Some(at) => at,
                None => {
                    let id = GamepadId::allocate();
                    let kind = controller.kind;
                    emit(GamepadEvent::Connected { id, kind });
                    self.slots.push(Slot {
                        key: controller.key,
                        id,
                        kind,
                        last: GamepadSnapshot::neutral(kind),
                    });
                    self.slots.len() - 1
                }
            };
            let slot = &mut self.slots[at];
            let snapshot = snapshot_of(&controller.reading, slot.kind);
            if snapshot != slot.last {
                slot.last = snapshot;
                emit(GamepadEvent::State {
                    id: slot.id,
                    snapshot,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionKind, ActionMap, Binding, PadButtons};

    /// The [`BUTTONS`] row reached by `path`.
    fn button_row(path: &[&CStr]) -> usize {
        BUTTONS
            .iter()
            .position(|&(row, _)| row == path)
            .unwrap_or_else(|| panic!("no button row for {path:?}"))
    }

    /// The [`AXES`] row reached by `path`.
    fn axis_row(path: &[&CStr]) -> usize {
        AXES.iter()
            .position(|&(row, _)| row == path)
            .unwrap_or_else(|| panic!("no axis row for {path:?}"))
    }

    /// A reading with only the element at `path` pressed.
    fn pressing(path: &[&CStr]) -> Reading {
        let mut reading = Reading::default();
        reading.pressed[button_row(path)] = true;
        reading
    }

    /// **The face buttons are positional by GameController's Xbox names**,
    /// and every other named element lands on its seam button — each alone.
    #[test]
    fn every_button_element_maps_to_its_position() {
        let expected: [(&[&CStr], PadButton); 15] = [
            (&[c"buttonA"], PadButton::South),
            (&[c"buttonB"], PadButton::East),
            (&[c"buttonX"], PadButton::West),
            (&[c"buttonY"], PadButton::North),
            (&[c"leftShoulder"], PadButton::LeftShoulder),
            (&[c"rightShoulder"], PadButton::RightShoulder),
            (&[c"leftThumbstickButton"], PadButton::LeftStick),
            (&[c"rightThumbstickButton"], PadButton::RightStick),
            (&[c"buttonMenu"], PadButton::Start),
            (&[c"buttonOptions"], PadButton::Select),
            (&[c"buttonHome"], PadButton::Guide),
            (&[c"dpad", c"up"], PadButton::DpadUp),
            (&[c"dpad", c"down"], PadButton::DpadDown),
            (&[c"dpad", c"left"], PadButton::DpadLeft),
            (&[c"dpad", c"right"], PadButton::DpadRight),
        ];
        for (path, button) in expected {
            assert_eq!(
                snapshot_of(&pressing(path), PadKind::Generic).buttons,
                [button].into_iter().collect::<PadButtons>(),
                "{path:?}"
            );
        }
        let every: PadButtons = PadButton::ALL.into_iter().collect();
        let all_pressed = Reading {
            pressed: [true; BUTTONS.len()],
            ..Reading::default()
        };
        assert_eq!(
            snapshot_of(&all_pressed, PadKind::Generic).buttons,
            every,
            "every seam button has an element"
        );
    }

    /// **The triggers are 0…1 from their `value`, left and right**, press no
    /// button, and **stick Y passes through unflipped** — GameController's
    /// +Y is already up.
    #[test]
    fn analog_elements_map_with_y_up_and_triggers_by_value() {
        let mut reading = Reading::default();
        reading.values[axis_row(&[c"leftTrigger"])] = 0.25;
        reading.values[axis_row(&[c"rightTrigger"])] = 0.75;
        reading.values[axis_row(&[c"leftThumbstick", c"xAxis"])] = 0.5;
        reading.values[axis_row(&[c"leftThumbstick", c"yAxis"])] = 1.0; // pushed up
        reading.values[axis_row(&[c"rightThumbstick", c"xAxis"])] = -0.25;
        reading.values[axis_row(&[c"rightThumbstick", c"yAxis"])] = -0.75; // pulled down
        let snapshot = snapshot_of(&reading, PadKind::Xbox);
        assert_eq!(snapshot.axis(PadAxis::LeftTrigger), 0.25);
        assert_eq!(snapshot.axis(PadAxis::RightTrigger), 0.75);
        assert_eq!(snapshot.axis(PadAxis::LeftX), 0.5);
        assert_eq!(snapshot.axis(PadAxis::LeftY), 1.0, "up is +Y");
        assert_eq!(snapshot.axis(PadAxis::RightX), -0.25);
        assert_eq!(snapshot.axis(PadAxis::RightY), -0.75, "down is -Y");
        assert!(snapshot.buttons.is_empty(), "a pulled trigger is an axis");
        assert_eq!(snapshot.kind, PadKind::Xbox);
        assert_eq!(
            snapshot_of(&Reading::default(), PadKind::Switch),
            GamepadSnapshot::neutral(PadKind::Switch)
        );
    }

    /// A value out of range clamps, and one that is not a number reads as
    /// rest, so every snapshot is finite.
    #[test]
    fn analog_values_are_clamped_and_finite() {
        let mut reading = Reading {
            values: [f32::NAN; AXES.len()],
            ..Reading::default()
        };
        reading.values[axis_row(&[c"leftTrigger"])] = 1.5;
        reading.values[axis_row(&[c"rightThumbstick", c"xAxis"])] = -3.0;
        let snapshot = snapshot_of(&reading, PadKind::Generic);
        assert!(snapshot.is_finite());
        assert_eq!(snapshot.axis(PadAxis::LeftTrigger), 1.0);
        assert_eq!(snapshot.axis(PadAxis::RightX), -1.0);
        assert_eq!(snapshot.axis(PadAxis::LeftY), 0.0, "NaN reads as rest");
    }

    /// The category names the family where it can, the vendor name where it
    /// cannot, and anything else is generic.
    #[test]
    fn the_category_or_vendor_names_the_family() {
        let cases = [
            (Some("DualSense"), None, PadKind::PlayStation),
            (Some("DualShock 4"), None, PadKind::PlayStation),
            (Some("Xbox One"), None, PadKind::Xbox),
            (Some("Switch Pro Controller"), None, PadKind::Switch),
            (
                Some("Nintendo Switch Joy-Con (L/R)"),
                Some("Joy-Con (L/R)"),
                PadKind::Switch,
            ),
            (Some("MFi"), Some("Xbox Wireless Controller"), PadKind::Xbox),
            (
                Some("HID"),
                Some("DUALSHOCK 4 Wireless"),
                PadKind::PlayStation,
            ),
            (None, Some("Pro Controller"), PadKind::Switch),
            (Some("Xbox One"), Some("DualSense"), PadKind::Xbox),
            (Some("MFi"), Some("SteelSeries Nimbus+"), PadKind::Generic),
            (Some("HID"), None, PadKind::Generic),
            (None, None, PadKind::Generic),
        ];
        for (category, vendor, kind) in cases {
            assert_eq!(kind_of(category, vendor), kind, "{category:?} / {vendor:?}");
        }
    }

    /// A list the tests script: each controller's key, family and reading.
    #[derive(Debug, Default)]
    struct Fake {
        listed: Vec<Seen>,
        reads: u32,
    }

    impl Fake {
        fn list(&mut self, key: usize, kind: PadKind, reading: Reading) {
            self.listed.push(Seen {
                key: Key(key),
                kind,
                reading,
            });
        }

        fn set(&mut self, key: usize, reading: Reading) {
            let controller = self
                .listed
                .iter_mut()
                .find(|controller| controller.key == Key(key))
                .expect("listed");
            controller.reading = reading;
        }

        fn unlist(&mut self, key: usize) {
            self.listed.retain(|controller| controller.key != Key(key));
        }
    }

    impl Source for Fake {
        fn read(&mut self, into: &mut Vec<Seen>) {
            self.reads += 1;
            into.clone_from(&self.listed);
        }
    }

    fn poll(poller: &mut Poller, fake: &mut Fake) -> Vec<GamepadEvent> {
        let mut events = Vec::new();
        poller.poll(fake, &mut |event| events.push(event));
        events
    }

    /// **Connect, change, idle, disconnect, reconnect**, each reported once
    /// and only when it happens, with a fresh id for a new controller object.
    #[test]
    fn controller_transitions_are_reported_once_each() {
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        assert_eq!(poll(&mut poller, &mut fake), [], "nothing listed");

        fake.list(0x1000, PadKind::PlayStation, pressing(&[c"buttonA"]));
        let events = poll(&mut poller, &mut fake);
        let [
            GamepadEvent::Connected { id, kind },
            GamepadEvent::State { id: same, snapshot },
        ] = events[..]
        else {
            panic!("a connection and its first state: {events:?}");
        };
        assert_eq!((kind, same), (PadKind::PlayStation, id));
        assert!(snapshot.buttons.contains(PadButton::South));
        assert_eq!(snapshot.kind, PadKind::PlayStation);

        assert_eq!(poll(&mut poller, &mut fake), [], "unchanged");
        assert_eq!(poll(&mut poller, &mut fake), [], "still unchanged");

        fake.set(0x1000, Reading::default());
        assert_eq!(
            poll(&mut poller, &mut fake),
            [GamepadEvent::State {
                id,
                snapshot: GamepadSnapshot::neutral(PadKind::PlayStation),
            }],
        );

        fake.unlist(0x1000);
        assert_eq!(
            poll(&mut poller, &mut fake),
            [GamepadEvent::Disconnected { id }]
        );
        assert_eq!(poll(&mut poller, &mut fake), [], "gone stays gone");

        fake.list(0x2000, PadKind::Xbox, Reading::default());
        let events = poll(&mut poller, &mut fake);
        let [GamepadEvent::Connected { id: again, kind }] = events[..] else {
            panic!("a reconnection at rest is one event: {events:?}");
        };
        assert_ne!(again, id, "a new controller object is a new pad");
        assert_eq!(kind, PadKind::Xbox);
        assert_eq!(fake.reads, 8, "the list is read on every poll");
    }

    /// **Pads are told apart by identity, not by position in the list**: the
    /// first of two leaving shifts the second to the front without
    /// reconnecting it, and a disconnection is emitted before the same poll's
    /// connection.
    #[test]
    fn identity_not_list_position_names_a_pad() {
        let mut poller = Poller::default();
        let mut fake = Fake::default();
        fake.list(1, PadKind::Xbox, Reading::default());
        fake.list(2, PadKind::Switch, Reading::default());
        let events = poll(&mut poller, &mut fake);
        let [
            GamepadEvent::Connected { id: first, .. },
            GamepadEvent::Connected { id: second, .. },
        ] = events[..]
        else {
            panic!("{events:?}");
        };

        fake.unlist(1);
        fake.list(3, PadKind::PlayStation, Reading::default());
        fake.set(2, pressing(&[c"buttonB"]));
        let events = poll(&mut poller, &mut fake);
        let [
            GamepadEvent::Disconnected { id: gone },
            GamepadEvent::State {
                id: pressed,
                snapshot,
            },
            GamepadEvent::Connected { id: third, kind },
        ] = events[..]
        else {
            panic!("{events:?}");
        };
        assert_eq!(gone, first);
        assert_eq!(pressed, second, "still the same pad at a new position");
        assert!(snapshot.buttons.contains(PadButton::East));
        assert_eq!(snapshot.kind, PadKind::Switch);
        assert_ne!(third, second);
        assert_eq!(kind, PadKind::PlayStation);
    }

    /// **The poller's events drive a binding** — the backend and the seam
    /// meeting, with no framework.
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
        fake.list(7, PadKind::Xbox, pressing(&[c"buttonA"]));
        poller.poll(&mut fake, &mut |event| map.gamepad_event(&event));
        assert!(map.just_pressed("jump"));

        fake.unlist(7);
        poller.poll(&mut fake, &mut |event| map.gamepad_event(&event));
        assert!(map.just_released("jump"), "a controller leaving releases");
    }
}
