//! What the W3C "standard" mapping's buttons and axes mean on the seam, and
//! what a `Gamepad.id` says about which family a pad is.
//!
//! Pure, and compiled into every target's tests: nothing here touches a
//! browser. The table itself is in the parent module's docs.

use crate::usb;
use crate::{GamepadSnapshot, PadAxis, PadButton, PadKind};

/// How many buttons the standard mapping names: `buttons[0]` to `buttons[16]`.
pub const STANDARD_BUTTONS: usize = 17;
/// How many axes it names: `axes[0]` to `axes[3]`.
pub const STANDARD_AXES: usize = 4;
/// How many values one report carries: every standard button's `value`, in
/// order, then every standard axis.
pub const VALUES: usize = STANDARD_BUTTONS + STANDARD_AXES;

/// `buttons[6]`, the left trigger: its `value` is the pull.
const LEFT_TRIGGER: usize = 6;
/// `buttons[7]`, the right trigger.
const RIGHT_TRIGGER: usize = 7;
/// `axes[0]`, the left stick's X, +right.
const LEFT_X: usize = STANDARD_BUTTONS;
/// `axes[1]`, the left stick's Y — **+down** in the standard mapping.
const LEFT_Y: usize = STANDARD_BUTTONS + 1;
/// `axes[2]`, the right stick's X, +right.
const RIGHT_X: usize = STANDARD_BUTTONS + 2;
/// `axes[3]`, the right stick's Y, +down like the left one.
const RIGHT_Y: usize = STANDARD_BUTTONS + 3;

/// Each standard button that is a seam button, by its index in
/// `Gamepad.buttons`. The two missing indices, 6 and 7, are the triggers,
/// which are axes on the seam.
const BUTTONS: [(usize, PadButton); 15] = [
    (0, PadButton::South),
    (1, PadButton::East),
    (2, PadButton::West),
    (3, PadButton::North),
    (4, PadButton::LeftShoulder),
    (5, PadButton::RightShoulder),
    (8, PadButton::Select),
    (9, PadButton::Start),
    (10, PadButton::LeftStick),
    (11, PadButton::RightStick),
    (12, PadButton::DpadUp),
    (13, PadButton::DpadDown),
    (14, PadButton::DpadLeft),
    (15, PadButton::DpadRight),
    (16, PadButton::Guide),
];

/// A stick axis as the seam holds it: −1…1, and 0 for a value that is not a
/// number at all.
///
/// The spec already bounds `Gamepad.axes` to −1…1, so the clamp only catches
/// a browser, or a shim, that strays; the seam's promise that every axis is
/// finite is not one to rest on someone else's. **No flip here** — which axes
/// are +down is the mapping's business, and the snapshot does it.
#[must_use]
pub fn stick_axis(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// A trigger's `GamepadButton.value` as the seam holds it: 0…1, and 0 for a
/// value that is not a number.
#[must_use]
pub fn trigger_axis(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// One standard-mapped report as the seam's snapshot.
///
/// `pressed` holds `buttons[i].pressed` in bit `i`, and `values` is laid out as
/// [`VALUES`] says. Buttons come from `pressed` — the browser's own threshold
/// for an analog face button — and the triggers from their `value`, so a
/// half-pulled trigger reads 0.5 rather than whatever side of a threshold it
/// sits on. Both stick Ys are negated: the standard mapping's +Y is down.
pub(super) fn snapshot_of(pressed: u32, values: &[f32; VALUES], kind: PadKind) -> GamepadSnapshot {
    let mut snapshot = GamepadSnapshot::neutral(kind);
    snapshot.buttons = BUTTONS
        .iter()
        .filter(|&&(index, _)| pressed & (1 << index) != 0)
        .map(|&(_, button)| button)
        .collect();
    snapshot.axes[PadAxis::LeftX as usize] = stick_axis(values[LEFT_X]);
    snapshot.axes[PadAxis::LeftY as usize] = -stick_axis(values[LEFT_Y]);
    snapshot.axes[PadAxis::RightX as usize] = stick_axis(values[RIGHT_X]);
    snapshot.axes[PadAxis::RightY as usize] = -stick_axis(values[RIGHT_Y]);
    snapshot.axes[PadAxis::LeftTrigger as usize] = trigger_axis(values[LEFT_TRIGGER]);
    snapshot.axes[PadAxis::RightTrigger as usize] = trigger_axis(values[RIGHT_TRIGGER]);
    snapshot
}

/// A pad's family from its `Gamepad.id`.
///
/// The spec leaves the string to the browser, and the browsers disagree:
///
/// - Chrome ends it with `(… Vendor: 054c Product: 0ce6)`;
/// - Firefox starts it with `054c-0ce6-`;
/// - Chrome on Windows names an XInput pad `Xbox 360 Controller (XInput
///   STANDARD GAMEPAD)` with no ids at all, and Safari gives only the name.
///
/// So the USB ids are read in either form and handed to the same table evdev
/// uses, and where there are none the name is matched against the families'
/// own product names. Anything else is [`PadKind::Generic`].
pub(super) fn kind_of_id(id: &str) -> PadKind {
    if let Some((vendor, product)) = usb_ids(id) {
        return usb::kind_of(vendor, product);
    }
    let name = id.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| name.contains(needle));
    if has(&["xinput", "xbox"]) {
        PadKind::Xbox
    } else if has(&["dualsense", "dualshock", "playstation"]) {
        PadKind::PlayStation
    } else if has(&["pro controller", "joy-con"]) {
        PadKind::Switch
    } else if has(&["steam deck"]) {
        PadKind::SteamDeck
    } else {
        PadKind::Generic
    }
}

/// The vendor and product ids in a `Gamepad.id`, in Chrome's or Firefox's
/// form — see [`kind_of_id`].
fn usb_ids(id: &str) -> Option<(u16, u16)> {
    if let Some(at) = id.find("Vendor: ") {
        let rest = &id[at + "Vendor: ".len()..];
        let vendor = hex_prefix(rest)?;
        let product_at = rest.find("Product: ")?;
        let product = hex_prefix(&rest[product_at + "Product: ".len()..])?;
        return Some((vendor, product));
    }
    let mut fields = id.splitn(3, '-');
    let vendor = fields.next().and_then(hex_field)?;
    let product = fields.next().and_then(hex_field)?;
    fields.next()?;
    Some((vendor, product))
}

/// The one to four hex digits `text` starts with, as a number.
fn hex_prefix(text: &str) -> Option<u16> {
    let digits = text
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(text.len());
    hex_field(&text[..digits])
}

/// `field` as hex, if it is one to four hex digits and nothing else.
fn hex_field(field: &str) -> Option<u16> {
    let valid = (1..=4).contains(&field.len()) && field.bytes().all(|b| b.is_ascii_hexdigit());
    valid.then(|| u16::from_str_radix(field, 16).ok()).flatten()
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::PadButtons;

    /// A report's values with every button out and every stick centred.
    pub(in crate::web_gamepad) const AT_REST: [f32; VALUES] = [0.0; VALUES];

    /// Every standard button index lands on its seam button, and on nothing
    /// else — the triggers included, which press no button at all.
    #[test]
    fn every_standard_button_maps_to_its_position() {
        for &(index, button) in &BUTTONS {
            let snapshot = snapshot_of(1 << index, &AT_REST, PadKind::Generic);
            assert_eq!(
                snapshot.buttons,
                [button].into_iter().collect::<PadButtons>(),
                "buttons[{index}]"
            );
        }
        assert_eq!(BUTTONS[14], (16, PadButton::Guide), "16 is the home button");
        for trigger in [LEFT_TRIGGER, RIGHT_TRIGGER] {
            let snapshot = snapshot_of(1 << trigger, &AT_REST, PadKind::Generic);
            assert!(snapshot.buttons.is_empty(), "buttons[{trigger}] is an axis");
        }
        let past_the_mapping = snapshot_of(1 << STANDARD_BUTTONS, &AT_REST, PadKind::Generic);
        assert!(past_the_mapping.buttons.is_empty(), "bit 17 names nothing");
    }

    /// **The triggers are buttons 6 and 7's `value`**, left and right, read
    /// whether or not the browser calls them pressed.
    #[test]
    fn the_triggers_are_buttons_six_and_seven_by_value() {
        let mut values = AT_REST;
        values[6] = 0.25;
        values[7] = 0.75;
        let snapshot = snapshot_of(0, &values, PadKind::Generic);
        assert_eq!(snapshot.axis(PadAxis::LeftTrigger), 0.25);
        assert_eq!(snapshot.axis(PadAxis::RightTrigger), 0.75);
        assert!(snapshot.buttons.is_empty());
    }

    /// **Axes 1 and 3 are +down and come out +up**; axes 0 and 2 are +right
    /// and stay so.
    #[test]
    fn stick_y_is_flipped_to_up() {
        let mut values = AT_REST;
        values[STANDARD_BUTTONS] = 0.5;
        values[STANDARD_BUTTONS + 1] = -1.0; // pushed up
        values[STANDARD_BUTTONS + 2] = -0.25;
        values[STANDARD_BUTTONS + 3] = 0.75; // pulled down
        let snapshot = snapshot_of(0, &values, PadKind::Generic);
        assert_eq!(snapshot.axis(PadAxis::LeftX), 0.5);
        assert_eq!(snapshot.axis(PadAxis::LeftY), 1.0, "up is +Y");
        assert_eq!(snapshot.axis(PadAxis::RightX), -0.25);
        assert_eq!(snapshot.axis(PadAxis::RightY), -0.75, "down is -Y");
        assert_eq!(snapshot_of(0, &AT_REST, PadKind::Xbox), {
            GamepadSnapshot::neutral(PadKind::Xbox)
        });
    }

    /// Out-of-range values clamp, and a value that is not a number reads as
    /// rest, so every snapshot is finite.
    #[test]
    fn values_are_clamped_and_finite() {
        assert_eq!(stick_axis(1.5), 1.0);
        assert_eq!(stick_axis(-7.0), -1.0);
        assert_eq!(stick_axis(f32::NAN), 0.0);
        assert_eq!(stick_axis(f32::NEG_INFINITY), 0.0);
        assert_eq!(stick_axis(-0.5), -0.5);
        assert_eq!(trigger_axis(-0.1), 0.0);
        assert_eq!(trigger_axis(1.01), 1.0);
        assert_eq!(trigger_axis(f32::INFINITY), 0.0);
        assert_eq!(trigger_axis(0.5), 0.5);
        let snapshot = snapshot_of(0, &[f32::NAN; VALUES], PadKind::Generic);
        assert!(snapshot.is_finite());
        assert_eq!(snapshot, GamepadSnapshot::neutral(PadKind::Generic));
    }

    /// Chrome's and Firefox's ids name the family by USB ids; an id with none
    /// falls back on the product name.
    #[test]
    fn the_id_names_the_family() {
        let cases = [
            (
                "DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)",
                PadKind::PlayStation,
            ),
            (
                "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)",
                PadKind::Xbox,
            ),
            (
                "Steam Deck (Vendor: 28de Product: 1205)",
                PadKind::SteamDeck,
            ),
            ("057e-2009-Pro Controller", PadKind::Switch),
            ("45e-28e-Xbox 360 Wired Controller", PadKind::Xbox),
            ("28de-11ff-Steam Virtual Gamepad", PadKind::Xbox),
            (
                "Xbox 360 Controller (XInput STANDARD GAMEPAD)",
                PadKind::Xbox,
            ),
            ("DualSense Wireless Controller", PadKind::PlayStation),
            ("Nintendo Switch Pro Controller", PadKind::Switch),
            (
                "Unknown Gamepad (Vendor: 1234 Product: 5678)",
                PadKind::Generic,
            ),
            ("Some Arcade Stick", PadKind::Generic),
            ("", PadKind::Generic),
        ];
        for (id, kind) in cases {
            assert_eq!(kind_of_id(id), kind, "{id:?}");
        }
    }

    /// A name that merely has hyphens in it is not read as ids.
    #[test]
    fn only_hex_fields_are_ids() {
        assert_eq!(usb_ids("Joy-Con (L)"), None);
        assert_eq!(usb_ids("054c-0ce6-DualSense"), Some((0x054c, 0x0ce6)));
        assert_eq!(usb_ids("12345-0ce6-Too long"), None);
        assert_eq!(usb_ids("054c-0ce6"), None, "no name after the ids");
        assert_eq!(
            usb_ids("Pad (Vendor: 054c Product: zz)"),
            None,
            "a vendor with no product"
        );
    }
}
