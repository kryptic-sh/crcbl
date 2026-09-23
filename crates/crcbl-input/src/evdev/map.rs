//! What an evdev device's codes mean on the seam: which devices are pads, which
//! key is which button, and how an axis's range becomes −1…1 or 0…1.
//!
//! Pure, and compiled into every target's tests: nothing here touches a device.
//! The table itself is in the parent module's docs.

use super::ffi::{
    ABS_BRAKE, ABS_CNT, ABS_GAS, ABS_HAT0X, ABS_HAT0Y, ABS_HAT2X, ABS_HAT2Y, ABS_RX, ABS_RY,
    ABS_RZ, ABS_X, ABS_Y, ABS_Z, AbsBits, AbsInfo, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT,
    BTN_DPAD_UP, BTN_EAST, BTN_GAMEPAD, BTN_MODE, BTN_NORTH, BTN_SELECT, BTN_SOUTH, BTN_START,
    BTN_THUMBL, BTN_THUMBR, BTN_TL, BTN_TL2, BTN_TR, BTN_TR2, BTN_WEST, InputId, KeyBits,
};
use crate::usb::{self, VENDOR_NINTENDO, VENDOR_SONY};
use crate::{GamepadSnapshot, PadAxis, PadButton, PadKind};

/// A stick axis from its `input_absinfo` range to −1…1, before any Y flip.
///
/// The centre is the range's midpoint rounded up, and **one scale serves both
/// halves** — the distance from the centre to `maximum` — with the value
/// clamped to −1…1. That is `xinput::stick_axis`'s rule
/// generalised: on xpad's −32768…32767 the centre is 0, 32767 reads 1 and the
/// spare −32768 clamps to −1, and a push of *n* codes reads the same distance
/// either way. A range with no extent (`maximum <= minimum`) reads 0.
///
/// `flat` is not applied: the seam's axes are raw (`gamepad.rs`), so the dead
/// zone is the binding's.
#[must_use]
pub fn stick_axis(value: i32, minimum: i32, maximum: i32) -> f32 {
    let (value, minimum, maximum) = (i64::from(value), i64::from(minimum), i64::from(maximum));
    let centre = minimum + (maximum - minimum + 1) / 2;
    let scale = maximum - centre;
    if scale <= 0 {
        return 0.0;
    }
    ((value - centre) as f64 / scale as f64).clamp(-1.0, 1.0) as f32
}

/// A trigger from its `input_absinfo` range to 0…1, `minimum` at rest,
/// clamped. A range with no extent reads 0.
#[must_use]
pub fn trigger_axis(value: i32, minimum: i32, maximum: i32) -> f32 {
    let (value, minimum, maximum) = (i64::from(value), i64::from(minimum), i64::from(maximum));
    let range = maximum - minimum;
    if range <= 0 {
        return 0.0;
    }
    ((value - minimum) as f64 / range as f64).clamp(0.0, 1.0) as f32
}

/// What the kernel says about a device, read once when it is opened and again
/// to resynchronise after `SYN_DROPPED`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Probe {
    /// `EVIOCGID`.
    pub(crate) id: InputId,
    /// `EVIOCGBIT(EV_KEY)`: the keys it has.
    pub(crate) keys: KeyBits,
    /// `EVIOCGBIT(EV_ABS)`: the axes it has.
    pub(crate) abs: AbsBits,
    /// `EVIOCGKEY`: the keys held now.
    pub(crate) held: KeyBits,
    /// `EVIOCGABS` for every axis in `abs`, with its value now; the default
    /// for an axis it does not have.
    pub(crate) axes: [AbsInfo; ABS_CNT],
}

impl Probe {
    /// Every axis's value now, indexed by code.
    pub(crate) fn values(&self) -> [i32; ABS_CNT] {
        self.axes.map(|info| info.value)
    }
}

/// **Whether a device is a gamepad**: it has `BTN_GAMEPAD` (which is
/// `BTN_SOUTH`) among its keys and `ABS_X` and `ABS_Y` among its axes — the
/// kernel's gamepad spec (`Documentation/input/gamepad.rst`) requires both of
/// a pad. A keyboard has neither; a touchpad and a flight stick have the axes
/// but not the button; the Deck's motion-sensor node has axes and no keys.
pub(crate) fn is_gamepad(keys: &KeyBits, abs: &AbsBits) -> bool {
    keys.has(BTN_GAMEPAD) && abs.has(ABS_X) && abs.has(ABS_Y)
}

/// A device's family, from its vendor and product ids — see `usb::kind_of`.
pub(crate) fn kind_of(id: InputId) -> PadKind {
    usb::kind_of(id.vendor, id.product)
}

/// The buttons every pad maps the same way.
const COMMON_BUTTONS: [(u16, PadButton); 9] = [
    (BTN_SOUTH, PadButton::South),
    (BTN_EAST, PadButton::East),
    (BTN_TL, PadButton::LeftShoulder),
    (BTN_TR, PadButton::RightShoulder),
    (BTN_THUMBL, PadButton::LeftStick),
    (BTN_THUMBR, PadButton::RightStick),
    (BTN_START, PadButton::Start),
    (BTN_SELECT, PadButton::Select),
    (BTN_MODE, PadButton::Guide),
];

/// The top and left face buttons for a driver that follows the gamepad spec's
/// positions: `hid-playstation`, `hid-sony`, `hid-nintendo`.
const POSITIONAL_FACE: [(u16, PadButton); 2] =
    [(BTN_NORTH, PadButton::North), (BTN_WEST, PadButton::West)];

/// The top and left face buttons for a driver that names them by the Xbox
/// letter instead: `xpad`, Steam Input's virtual pad and `hid-steam` report X
/// as `BTN_X` and Y as `BTN_Y`, and the header aliases those to `BTN_NORTH`
/// and `BTN_WEST` — so on these pads the code called north is the left button.
const LETTERED_FACE: [(u16, PadButton); 2] =
    [(BTN_NORTH, PadButton::West), (BTN_WEST, PadButton::North)];

/// The d-pad as keys, on a device that has them.
const DPAD_KEYS: [(u16, PadButton); 4] = [
    (BTN_DPAD_UP, PadButton::DpadUp),
    (BTN_DPAD_DOWN, PadButton::DpadDown),
    (BTN_DPAD_LEFT, PadButton::DpadLeft),
    (BTN_DPAD_RIGHT, PadButton::DpadRight),
];

/// Each stick axis's code.
const STICKS: [(u16, PadAxis); 4] = [
    (ABS_X, PadAxis::LeftX),
    (ABS_Y, PadAxis::LeftY),
    (ABS_RX, PadAxis::RightX),
    (ABS_RY, PadAxis::RightY),
];

/// Each trigger: the analog axes that may carry it, first match wins, and the
/// key that stands in for it on a pad with a digital trigger (a Switch Pro's
/// ZL and ZR). `ABS_Z`/`ABS_RZ` is xpad's and the PlayStation drivers',
/// `ABS_BRAKE`/`ABS_GAS` Bluetooth Xbox pads', and `ABS_HAT2Y`/`ABS_HAT2X`
/// `hid-steam`'s.
const TRIGGERS: [([u16; 3], u16, PadAxis); 2] = [
    ([ABS_Z, ABS_BRAKE, ABS_HAT2Y], BTN_TL2, PadAxis::LeftTrigger),
    ([ABS_RZ, ABS_GAS, ABS_HAT2X], BTN_TR2, PadAxis::RightTrigger),
];

/// How far a hat axis must be from centre, after [`stick_axis`], to press
/// that d-pad direction: halfway, so a −1…1 hat presses at ±1 and a wider
/// range is not pressed by noise.
const HAT_PRESS: f32 = 0.5;

/// What one axis code drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// A stick or trigger axis.
    Axis(PadAxis),
    /// The d-pad's left and right, from a hat.
    HatX,
    /// The d-pad's up and down, from a hat; −1 is up.
    HatY,
}

/// How one device's codes land on a [`GamepadSnapshot`], worked out once from
/// its [`Probe`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Layout {
    /// The device's family.
    pub(crate) kind: PadKind,
    /// The keys it has that are buttons.
    buttons: Vec<(u16, PadButton)>,
    /// The axes it has that the seam reads, with their ranges.
    axes: Vec<(u16, Role, AbsInfo)>,
    /// Keys that stand in for a trigger it has no axis for.
    digital_triggers: Vec<(u16, PadAxis)>,
}

impl Layout {
    /// The layout of the device `probe` describes.
    ///
    /// The d-pad comes from the `BTN_DPAD_*` keys on a device that has any,
    /// and from `ABS_HAT0X`/`ABS_HAT0Y` otherwise. Not both: `hid-steam`
    /// reports the Deck's d-pad as keys and its left trackpad on the hat
    /// axes, and a touch on the pad is not a d-pad press.
    pub(crate) fn new(probe: &Probe) -> Self {
        let kind = kind_of(probe.id);
        let has_key = |code: u16| probe.keys.has(code);
        let has_abs = |code: u16| probe.abs.has(code);
        let face = match probe.id.vendor {
            VENDOR_SONY | VENDOR_NINTENDO => POSITIONAL_FACE,
            _ => LETTERED_FACE,
        };
        let dpad_keys = DPAD_KEYS.iter().any(|&(code, _)| has_key(code));
        let dpad: &[(u16, PadButton)] = if dpad_keys { &DPAD_KEYS } else { &[] };
        let buttons = COMMON_BUTTONS
            .iter()
            .chain(&face)
            .chain(dpad)
            .copied()
            .filter(|&(code, _)| has_key(code))
            .collect();

        let info = |code: u16| probe.axes[usize::from(code)];
        let mut axes: Vec<_> = STICKS
            .iter()
            .filter(|&&(code, _)| has_abs(code))
            .map(|&(code, axis)| (code, Role::Axis(axis), info(code)))
            .collect();
        let mut digital_triggers = Vec::new();
        for (candidates, key, axis) in TRIGGERS {
            match candidates.into_iter().find(|&code| has_abs(code)) {
                Some(code) => axes.push((code, Role::Axis(axis), info(code))),
                None if has_key(key) => digital_triggers.push((key, axis)),
                None => {}
            }
        }
        if !dpad_keys {
            for (code, role) in [(ABS_HAT0X, Role::HatX), (ABS_HAT0Y, Role::HatY)] {
                if has_abs(code) {
                    axes.push((code, role, info(code)));
                }
            }
        }
        Self {
            kind,
            buttons,
            axes,
            digital_triggers,
        }
    }

    /// The snapshot a device in this layout is showing, from the keys it
    /// holds and every axis's value. **evdev's +Y is down**, so both stick Y
    /// axes are negated onto the seam's +Y up.
    pub(crate) fn snapshot(&self, held: &KeyBits, values: &[i32; ABS_CNT]) -> GamepadSnapshot {
        let mut snapshot = GamepadSnapshot::neutral(self.kind);
        snapshot.buttons = self
            .buttons
            .iter()
            .filter(|&&(code, _)| held.has(code))
            .map(|&(_, button)| button)
            .collect();
        for &(code, axis) in &self.digital_triggers {
            if held.has(code) {
                snapshot.axes[axis as usize] = 1.0;
            }
        }
        for &(code, role, info) in &self.axes {
            let value = values[usize::from(code)];
            let stick = stick_axis(value, info.minimum, info.maximum);
            match role {
                Role::Axis(axis @ (PadAxis::LeftTrigger | PadAxis::RightTrigger)) => {
                    snapshot.axes[axis as usize] = trigger_axis(value, info.minimum, info.maximum);
                }
                Role::Axis(axis @ (PadAxis::LeftY | PadAxis::RightY)) => {
                    snapshot.axes[axis as usize] = -stick;
                }
                Role::Axis(axis) => snapshot.axes[axis as usize] = stick,
                Role::HatX if stick <= -HAT_PRESS => snapshot.buttons.insert(PadButton::DpadLeft),
                Role::HatX if stick >= HAT_PRESS => snapshot.buttons.insert(PadButton::DpadRight),
                Role::HatY if stick <= -HAT_PRESS => snapshot.buttons.insert(PadButton::DpadUp),
                Role::HatY if stick >= HAT_PRESS => snapshot.buttons.insert(PadButton::DpadDown),
                Role::HatX | Role::HatY => {}
            }
        }
        snapshot
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::PadButtons;
    use crate::usb::{PRODUCT_STEAM_DECK, VENDOR_MICROSOFT, VENDOR_VALVE};

    pub(crate) const XBOX: InputId = InputId {
        bustype: 3,
        vendor: VENDOR_MICROSOFT,
        product: 0x028e,
        version: 0,
    };
    const SONY: InputId = InputId {
        bustype: 3,
        vendor: VENDOR_SONY,
        product: 0x0ce6,
        version: 0,
    };

    /// xpad's stick range.
    pub(crate) const XPAD_STICK: (i32, i32) = (-32768, 32767);
    /// xpad's trigger range.
    pub(crate) const XPAD_TRIGGER: (i32, i32) = (0, 255);

    /// A device probe: `id`, the `keys` it has, and the `axes` it has with
    /// their `(minimum, maximum)`, every axis at rest — a trigger at its
    /// minimum, anything else at the centre of its range — and nothing held.
    pub(crate) fn probe(id: InputId, keys: &[u16], axes: &[(u16, (i32, i32))]) -> Probe {
        let is_trigger = |code| TRIGGERS.iter().any(|(axes, _, _)| axes.contains(&code));
        let mut probe = Probe {
            id,
            keys: KeyBits::EMPTY,
            abs: AbsBits::EMPTY,
            held: KeyBits::EMPTY,
            axes: [AbsInfo::default(); ABS_CNT],
        };
        for &code in keys {
            probe.keys.set(code, true);
        }
        for &(code, (minimum, maximum)) in axes {
            probe.abs.set(code, true);
            probe.axes[usize::from(code)] = AbsInfo {
                value: if is_trigger(code) {
                    minimum
                } else {
                    minimum + (maximum - minimum + 1) / 2
                },
                minimum,
                maximum,
                flat: 128,
                ..AbsInfo::default()
            };
        }
        probe
    }

    /// Every key an Xbox 360 pad has under xpad.
    pub(crate) const XPAD_KEYS: [u16; 11] = [
        BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_TL, BTN_TR, BTN_SELECT, BTN_START, BTN_MODE,
        BTN_THUMBL, BTN_THUMBR,
    ];

    /// An Xbox 360 pad under xpad: sticks, `ABS_Z`/`ABS_RZ` triggers, a hat.
    pub(crate) fn xpad() -> Probe {
        probe(
            XBOX,
            &XPAD_KEYS,
            &[
                (ABS_X, XPAD_STICK),
                (ABS_Y, XPAD_STICK),
                (ABS_RX, XPAD_STICK),
                (ABS_RY, XPAD_STICK),
                (ABS_Z, XPAD_TRIGGER),
                (ABS_RZ, XPAD_TRIGGER),
                (ABS_HAT0X, (-1, 1)),
                (ABS_HAT0Y, (-1, 1)),
            ],
        )
    }

    /// `probe`'s snapshot with `keys` held and `values` on the axes that
    /// name one, every other axis where the probe left it.
    fn snapshot(probe: &Probe, keys: &[u16], values: &[(u16, i32)]) -> GamepadSnapshot {
        let mut held = KeyBits::EMPTY;
        for &code in keys {
            held.set(code, true);
        }
        let mut current = probe.values();
        for &(code, value) in values {
            current[usize::from(code)] = value;
        }
        Layout::new(probe).snapshot(&held, &current)
    }

    fn buttons(list: &[PadButton]) -> PadButtons {
        list.iter().copied().collect()
    }

    /// Hand-worked values at both ends and the centre of the ranges real
    /// drivers declare.
    #[test]
    fn stick_values_normalise_against_known_ranges() {
        // xpad: −32768…32767, centre 0, one scale of 32767.
        assert_eq!(
            stick_axis(-32768, -32768, 32767),
            -1.0,
            "the spare code clamps"
        );
        assert_eq!(stick_axis(-32767, -32768, 32767), -1.0);
        assert_eq!(stick_axis(0, -32768, 32767), 0.0);
        assert_eq!(stick_axis(32767, -32768, 32767), 1.0);
        assert_eq!(
            stick_axis(-16384, -32768, 32767),
            -stick_axis(16384, -32768, 32767),
            "one scale both ways"
        );
        // hid-steam's Deck sticks: symmetric −32767…32767.
        assert_eq!(stick_axis(-32767, -32767, 32767), -1.0);
        assert_eq!(stick_axis(0, -32767, 32767), 0.0);
        // A byte-wide HID stick: 0…255, centre 128, scale 127.
        assert_eq!(stick_axis(128, 0, 255), 0.0);
        assert_eq!(stick_axis(255, 0, 255), 1.0);
        assert_eq!(stick_axis(1, 0, 255), -1.0);
        assert_eq!(stick_axis(0, 0, 255), -1.0, "the spare code clamps");
        assert!((stick_axis(192, 0, 255) - 64.0 / 127.0).abs() < f32::EPSILON);
        // A hat, and values a driver reports past its own range.
        assert_eq!(stick_axis(-1, -1, 1), -1.0);
        assert_eq!(stick_axis(40000, -32768, 32767), 1.0);
        // No extent: nothing to scale by.
        assert_eq!(stick_axis(5, 5, 5), 0.0);
        assert_eq!(stick_axis(5, 10, 0), 0.0);
        assert_eq!(stick_axis(i32::MAX, i32::MIN, i32::MAX), 1.0, "no overflow");
    }

    #[test]
    fn trigger_values_normalise_against_known_ranges() {
        assert_eq!(trigger_axis(0, 0, 255), 0.0);
        assert_eq!(trigger_axis(255, 0, 255), 1.0);
        assert!((trigger_axis(128, 0, 255) - 128.0 / 255.0).abs() < f32::EPSILON);
        assert_eq!(trigger_axis(1023, 0, 1023), 1.0, "Xbox One's ten bits");
        assert_eq!(trigger_axis(32767, 0, 32767), 1.0, "hid-steam's");
        assert_eq!(trigger_axis(-5, 0, 255), 0.0, "clamped");
        assert_eq!(trigger_axis(9, 9, 9), 0.0, "no extent");
    }

    /// **evdev's +Y is down**: a stick pushed to its maximum Y is pulled
    /// toward the player, and reads −1 on the seam; X is not flipped; and
    /// `flat` does not zero a small push, since the seam's axes are raw.
    #[test]
    fn stick_y_is_flipped_to_up_and_flat_is_not_applied() {
        let pad = xpad();
        let pushed_up = snapshot(&pad, &[], &[(ABS_Y, -32768), (ABS_RY, -32768)]);
        assert_eq!(pushed_up.axis(PadAxis::LeftY), 1.0, "evdev −Y is up");
        assert_eq!(pushed_up.axis(PadAxis::RightY), 1.0);
        let pulled_down = snapshot(&pad, &[], &[(ABS_Y, 32767), (ABS_RY, 32767)]);
        assert_eq!(pulled_down.axis(PadAxis::LeftY), -1.0);
        assert_eq!(pulled_down.axis(PadAxis::RightY), -1.0);
        let right = snapshot(&pad, &[], &[(ABS_X, 32767), (ABS_RX, -32768)]);
        assert_eq!(right.axis(PadAxis::LeftX), 1.0, "+X right, unflipped");
        assert_eq!(right.axis(PadAxis::RightX), -1.0);

        let inside_flat = snapshot(&pad, &[], &[(ABS_X, 64)]);
        assert_eq!(pad.axes[usize::from(ABS_X)].flat, 128);
        assert!((inside_flat.axis(PadAxis::LeftX) - 64.0 / 32767.0).abs() < f32::EPSILON);

        let at_rest = snapshot(&pad, &[], &[]);
        assert_eq!(at_rest, GamepadSnapshot::neutral(PadKind::Xbox));
    }

    /// Every button code lands on its position, on a lettered (Xbox) driver
    /// and on a positional (PlayStation) one.
    #[test]
    fn buttons_map_by_position_for_each_driver_family() {
        let lettered = xpad();
        for (code, button) in [
            (BTN_SOUTH, PadButton::South),
            (BTN_EAST, PadButton::East),
            (BTN_NORTH, PadButton::West),
            (BTN_WEST, PadButton::North),
            (BTN_TL, PadButton::LeftShoulder),
            (BTN_TR, PadButton::RightShoulder),
            (BTN_THUMBL, PadButton::LeftStick),
            (BTN_THUMBR, PadButton::RightStick),
            (BTN_START, PadButton::Start),
            (BTN_SELECT, PadButton::Select),
            (BTN_MODE, PadButton::Guide),
        ] {
            assert_eq!(
                snapshot(&lettered, &[code], &[]).buttons,
                buttons(&[button]),
                "xpad {code:#x}"
            );
        }

        let positional = probe(
            SONY,
            &XPAD_KEYS,
            &[(ABS_X, XPAD_TRIGGER), (ABS_Y, XPAD_TRIGGER)],
        );
        assert_eq!(
            snapshot(&positional, &[BTN_NORTH], &[]).buttons,
            buttons(&[PadButton::North]),
            "hid-playstation's triangle"
        );
        assert_eq!(
            snapshot(&positional, &[BTN_WEST], &[]).buttons,
            buttons(&[PadButton::West]),
            "hid-playstation's square"
        );
        assert_eq!(snapshot(&positional, &[], &[]).kind, PadKind::PlayStation);
    }

    /// The hat presses each d-pad direction at its ends, diagonals press two,
    /// and −1 on the Y hat is up.
    #[test]
    fn the_hat_maps_onto_the_dpad() {
        let pad = xpad();
        for ((x, y), expected) in [
            ((0, -1), &[PadButton::DpadUp][..]),
            ((0, 1), &[PadButton::DpadDown]),
            ((-1, 0), &[PadButton::DpadLeft]),
            ((1, 0), &[PadButton::DpadRight]),
            ((1, -1), &[PadButton::DpadRight, PadButton::DpadUp]),
            ((0, 0), &[]),
        ] {
            assert_eq!(
                snapshot(&pad, &[], &[(ABS_HAT0X, x), (ABS_HAT0Y, y)]).buttons,
                buttons(expected),
                "hat at ({x}, {y})"
            );
        }
    }

    /// A device with `BTN_DPAD_*` keys has its d-pad read from them, and its
    /// hat axes — `hid-steam`'s left trackpad on the Deck — press nothing.
    #[test]
    fn dpad_keys_win_over_the_hat() {
        let deck = probe(
            InputId {
                vendor: VENDOR_VALVE,
                product: PRODUCT_STEAM_DECK,
                ..XBOX
            },
            &[
                BTN_SOUTH,
                BTN_DPAD_UP,
                BTN_DPAD_DOWN,
                BTN_DPAD_LEFT,
                BTN_DPAD_RIGHT,
            ],
            &[
                (ABS_X, (-32767, 32767)),
                (ABS_Y, (-32767, 32767)),
                (ABS_HAT0X, (-32767, 32767)),
                (ABS_HAT0Y, (-32767, 32767)),
            ],
        );
        for (code, button) in DPAD_KEYS {
            assert_eq!(snapshot(&deck, &[code], &[]).buttons, buttons(&[button]));
        }
        let touched = snapshot(&deck, &[], &[(ABS_HAT0X, 32767), (ABS_HAT0Y, -32767)]);
        assert!(touched.buttons.is_empty(), "{touched:?}");
        assert_eq!(touched.kind, PadKind::SteamDeck);
    }

    /// Each trigger reads its first axis the device has, in the order
    /// `TRIGGERS` lists, and a digital trigger key stands in for a missing
    /// axis.
    #[test]
    fn triggers_come_from_the_first_axis_present() {
        let sticks = [(ABS_X, XPAD_STICK), (ABS_Y, XPAD_STICK)];
        let both = probe(
            XBOX,
            &[BTN_SOUTH],
            &[
                sticks[0],
                sticks[1],
                (ABS_Z, XPAD_TRIGGER),
                (ABS_RZ, XPAD_TRIGGER),
                (ABS_BRAKE, (0, 1023)),
                (ABS_GAS, (0, 1023)),
            ],
        );
        let pulled = snapshot(&both, &[], &[(ABS_Z, 255), (ABS_BRAKE, 0), (ABS_GAS, 1023)]);
        assert_eq!(pulled.axis(PadAxis::LeftTrigger), 1.0, "ABS_Z first");
        assert_eq!(pulled.axis(PadAxis::RightTrigger), 0.0, "ABS_GAS ignored");

        let bluetooth = probe(
            XBOX,
            &[BTN_SOUTH],
            &[
                sticks[0],
                sticks[1],
                (ABS_BRAKE, (0, 1023)),
                (ABS_GAS, (0, 1023)),
            ],
        );
        let pulled = snapshot(&bluetooth, &[], &[(ABS_BRAKE, 1023), (ABS_GAS, 0)]);
        assert_eq!(pulled.axis(PadAxis::LeftTrigger), 1.0, "brake is left");
        assert_eq!(pulled.axis(PadAxis::RightTrigger), 0.0);

        let steam = probe(
            XBOX,
            &[BTN_SOUTH],
            &[
                sticks[0],
                sticks[1],
                (ABS_HAT2Y, (0, 32767)),
                (ABS_HAT2X, (0, 32767)),
            ],
        );
        let pulled = snapshot(&steam, &[], &[(ABS_HAT2X, 32767), (ABS_HAT2Y, 0)]);
        assert_eq!(pulled.axis(PadAxis::RightTrigger), 1.0, "HAT2X is right");
        assert_eq!(pulled.axis(PadAxis::LeftTrigger), 0.0);

        let digital = probe(
            InputId {
                vendor: VENDOR_NINTENDO,
                ..XBOX
            },
            &[BTN_SOUTH, BTN_TL2, BTN_TR2],
            &sticks,
        );
        let pulled = snapshot(&digital, &[BTN_TR2], &[]);
        assert_eq!(pulled.axis(PadAxis::RightTrigger), 1.0, "ZR held");
        assert_eq!(pulled.axis(PadAxis::LeftTrigger), 0.0);
        assert!(pulled.buttons.is_empty(), "a trigger, not a button");
    }

    /// **A keyboard is not a pad**, nor is anything else with only half of
    /// what the spec requires.
    #[test]
    fn capability_bits_classify_pads() {
        let class = |probe: &Probe| is_gamepad(&probe.keys, &probe.abs);
        assert!(class(&xpad()), "an Xbox pad");
        const KEY_ESC: u16 = 1;
        const KEY_A: u16 = 30;
        const KEY_SPACE: u16 = 57;
        const BTN_LEFT: u16 = 0x110;
        const BTN_TRIGGER: u16 = 0x120;
        const BTN_TOUCH: u16 = 0x14a;
        let axes = [(ABS_X, XPAD_STICK), (ABS_Y, XPAD_STICK)];
        for (what, keys, axes) in [
            ("a keyboard", &[KEY_ESC, KEY_A, KEY_SPACE][..], &[][..]),
            ("a touchpad", &[BTN_LEFT, BTN_TOUCH], &axes[..]),
            ("a flight stick", &[BTN_TRIGGER], &axes),
            (
                "motion sensors",
                &[],
                &[
                    (ABS_X, XPAD_STICK),
                    (ABS_Y, XPAD_STICK),
                    (ABS_Z, XPAD_STICK),
                ],
            ),
            ("buttons with no stick", &[BTN_SOUTH, BTN_EAST], &[]),
            ("a pad missing ABS_Y", &[BTN_SOUTH], &[(ABS_X, XPAD_STICK)]),
        ] {
            assert!(!class(&probe(XBOX, keys, axes)), "{what} is not a pad");
        }
    }

    #[test]
    fn vendor_and_product_name_the_family() {
        let id = |vendor, product| InputId {
            vendor,
            product,
            ..XBOX
        };
        assert_eq!(kind_of(id(VENDOR_MICROSOFT, 0x028e)), PadKind::Xbox);
        assert_eq!(
            kind_of(id(VENDOR_VALVE, PRODUCT_STEAM_DECK)),
            PadKind::SteamDeck
        );
        assert_eq!(
            kind_of(id(VENDOR_VALVE, 0x11ff)),
            PadKind::Xbox,
            "Steam's virtual pad"
        );
        assert_eq!(kind_of(id(VENDOR_SONY, 0x0ce6)), PadKind::PlayStation);
        assert_eq!(kind_of(id(VENDOR_NINTENDO, 0x2009)), PadKind::Switch);
        assert_eq!(kind_of(id(0x1234, 0x5678)), PadKind::Generic);
    }
}
