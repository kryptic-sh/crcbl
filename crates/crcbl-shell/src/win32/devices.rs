//! Which physical keyboard or mouse an input message came from.
//!
//! Pure, and for the reason [`pointer`](super::pointer) states: the matching
//! below is exactly the kind of arithmetic that is wrong-but-plausible, and it
//! is checked by `cargo test` on every host rather than only by a desk with two
//! keyboards.
//!
//! # The messages are the events; raw input says whose they are
//!
//! A `WM_KEYDOWN` or `WM_LBUTTONDOWN` names no device. Replacing those messages
//! with raw input would name one, and would lose what only they carry: routing to
//! the focused and capturing window, `TranslateMessage` building `WM_CHAR`,
//! client coordinates, and delivery through the modal loops Windows runs itself.
//! So the legacy messages stay the source of every key, button and wheel event,
//! and each one takes its [`DeviceId`] from the **raw report that produced it**.
//! Both the mouse and the keyboard are registered for raw input without
//! `RIDEV_NOLEGACY`, so every keystroke and click arrives twice: once as a
//! `WM_INPUT` carrying `hDevice`, once as the ordinary message.
//!
//! # Matching a message to its report
//!
//! `GetMessage` hands out posted messages before input messages, so a report
//! is normally recorded before the message it produced. It is not always the
//! next report, though: two devices typing inside one pump interleave, and a
//! report whose message the system consumed itself (Alt+Tab) never gets one. So
//! [`Attribution`] keeps the reports in arrival order and a message takes the
//! **oldest one with the same content**, meaning the same scan code and direction,
//! or the same button edge, or the same wheel axis, whose timestamp is within
//! [`MATCH_WINDOW_MS`] of the message's. Reports that nothing claims age out.
//! A message with no matching report, injected input for one (`SendInput`
//! reports carry a null `hDevice`), falls back to the per-kind constant.
//!
//! Pointer motion has no content to match, so it takes the device of the most
//! recent mouse report. The raw delta itself is exact: it comes from the
//! report, not from a match.
//!
//! # Ids are keyed by interface path
//!
//! [`DeviceId`] promises an id that is unique for the process and never handed
//! to another device. [`DeviceTable`] keys ids by the device's interface path
//! (`RIDI_DEVICENAME`), which outlives the handle: a keyboard unplugged and
//! plugged back in gets a new handle and keeps its id, which is what a
//! local-multiplayer seat assignment wants. A handle is forgotten when
//! `WM_INPUT_DEVICE_CHANGE` says its device left, because Windows may give the
//! same handle value to the next device that arrives.

use std::collections::VecDeque;

use crcbl_core::input::{ButtonState, DeviceId, PointerButton};

use super::ffi::value;

/// The device a key with no attributable source carries: injected input, or a
/// report that was never matched.
pub const KEYBOARD_DEVICE: DeviceId = DeviceId(1);
/// The device a pointer event with no attributable source carries.
pub const POINTER_DEVICE: DeviceId = DeviceId(2);
/// The device every touch contact carries. The contact id is what tells fingers
/// apart; `WM_POINTER*` does not name the touchscreen.
pub const TOUCH_DEVICE: DeviceId = DeviceId(3);
/// The first id [`DeviceTable`] hands a real device, above the per-kind ones.
const FIRST_REAL_DEVICE: u32 = TOUCH_DEVICE.0 + 1;

/// How far apart, in `GetMessageTime` milliseconds, a raw report and the
/// message it produced may be. Both are stamped by the same input thread for
/// the same event, so in practice they agree; the window only has to be long
/// enough to survive a busy frame.
pub const MATCH_WINDOW_MS: u32 = 500;
/// The most reports kept waiting for their message. A bound on memory, not on
/// correctness: [`MATCH_WINDOW_MS`] is what retires a report normally.
const MAX_PENDING: usize = 64;

/// What a raw report says happened, in the terms a legacy message can be
/// compared against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    /// A key, by the scan code `keys::scancode` would fold from the message.
    Key { scancode: u32, pressed: bool },
    /// A button going down or up.
    Button {
        button: PointerButton,
        pressed: bool,
    },
    /// A wheel detent on one axis.
    Wheel { horizontal: bool },
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    edge: Edge,
    device: DeviceId,
    millis: u32,
}

/// Whether two `GetMessageTime` stamps are within [`MATCH_WINDOW_MS`] of each
/// other, across the 49.7-day wrap.
const fn close(a: u32, b: u32) -> bool {
    a.wrapping_sub(b) <= MATCH_WINDOW_MS || b.wrapping_sub(a) <= MATCH_WINDOW_MS
}

/// The scan code a raw keyboard report describes, folded the way
/// `keys::scancode` folds a message's `lParam`: the `E0` prefix becomes
/// `0xE000`. `None` for an `E1`-prefixed report (Pause), whose message carries a
/// different code; it is left to the fallback.
#[must_use]
pub const fn raw_scancode(make_code: u16, flags: u16) -> Option<u32> {
    if flags & value::RI_KEY_E1 != 0 {
        return None;
    }
    let prefix = if flags & value::RI_KEY_E0 != 0 {
        0xE000
    } else {
        0
    };
    Some(prefix | make_code as u32)
}

/// Every button and wheel edge a raw mouse report's `usButtonFlags` carries.
fn mouse_edges(flags: u16) -> impl Iterator<Item = Edge> {
    const BUTTONS: [(u16, PointerButton); 5] = [
        (value::RI_MOUSE_LEFT_BUTTON_DOWN, PointerButton::Left),
        (value::RI_MOUSE_RIGHT_BUTTON_DOWN, PointerButton::Right),
        (value::RI_MOUSE_MIDDLE_BUTTON_DOWN, PointerButton::Middle),
        (value::RI_MOUSE_BUTTON_4_DOWN, PointerButton::Back),
        (value::RI_MOUSE_BUTTON_5_DOWN, PointerButton::Forward),
    ];
    // Each button's up edge is the bit after its down edge.
    let buttons = BUTTONS.into_iter().flat_map(move |(down, button)| {
        [(down, true), (down << 1, false)]
            .into_iter()
            .filter(move |&(bit, _)| flags & bit != 0)
            .map(move |(_, pressed)| Edge::Button { button, pressed })
    });
    let wheels = [
        (value::RI_MOUSE_WHEEL, false),
        (value::RI_MOUSE_H_WHEEL, true),
    ]
    .into_iter()
    .filter(move |&(bit, _)| flags & bit != 0)
    .map(|(_, horizontal)| Edge::Wheel { horizontal });
    buttons.chain(wheels)
}

/// Raw reports waiting for the legacy messages they produced.
#[derive(Debug)]
pub struct Attribution {
    pending: VecDeque<Pending>,
    /// The device of the most recent mouse report, which pointer motion takes.
    pointer: DeviceId,
}

impl Default for Attribution {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            pointer: POINTER_DEVICE,
        }
    }
}

impl Attribution {
    /// Records a raw keyboard report. `device` is `None` for injected input.
    pub fn raw_key(&mut self, scancode: u32, pressed: bool, device: Option<DeviceId>, millis: u32) {
        let device = device.unwrap_or(KEYBOARD_DEVICE);
        self.push(Edge::Key { scancode, pressed }, device, millis);
    }

    /// Records a raw mouse report: its device becomes the pointer's, and each
    /// button or wheel edge it carries waits for its message.
    pub fn raw_mouse(&mut self, button_flags: u16, device: Option<DeviceId>, millis: u32) {
        let device = device.unwrap_or(POINTER_DEVICE);
        self.pointer = device;
        for edge in mouse_edges(button_flags) {
            self.push(edge, device, millis);
        }
    }

    /// The device a `WM_KEYDOWN`/`WM_KEYUP` came from.
    pub fn key(&mut self, scancode: u32, state: ButtonState, millis: u32) -> DeviceId {
        let pressed = state.is_pressed();
        self.take(Edge::Key { scancode, pressed }, millis)
            .unwrap_or(KEYBOARD_DEVICE)
    }

    /// The device a button message came from.
    pub fn button(&mut self, button: PointerButton, state: ButtonState, millis: u32) -> DeviceId {
        let pressed = state.is_pressed();
        self.take(Edge::Button { button, pressed }, millis)
            .unwrap_or(self.pointer)
    }

    /// The device a wheel message came from.
    pub fn wheel(&mut self, horizontal: bool, millis: u32) -> DeviceId {
        self.take(Edge::Wheel { horizontal }, millis)
            .unwrap_or(self.pointer)
    }

    /// The device pointer motion is attributed to: the latest mouse report's.
    #[must_use]
    pub const fn pointer(&self) -> DeviceId {
        self.pointer
    }

    fn push(&mut self, edge: Edge, device: DeviceId, millis: u32) {
        self.retire(millis);
        if self.pending.len() == MAX_PENDING {
            self.pending.pop_front();
        }
        self.pending.push_back(Pending {
            edge,
            device,
            millis,
        });
    }

    fn take(&mut self, edge: Edge, millis: u32) -> Option<DeviceId> {
        self.retire(millis);
        let index = self
            .pending
            .iter()
            .position(|pending| pending.edge == edge && close(pending.millis, millis))?;
        self.pending.remove(index).map(|pending| pending.device)
    }

    /// Drops the reports too old to be claimed by a message stamped `now`.
    fn retire(&mut self, now: u32) {
        while self
            .pending
            .front()
            .is_some_and(|oldest| !close(oldest.millis, now))
        {
            self.pending.pop_front();
        }
    }
}

/// Raw input handles, and the stable ids behind them.
#[derive(Debug)]
pub struct DeviceTable {
    handles: Vec<(isize, DeviceId)>,
    /// Interface path → id, never pruned, so a replugged device keeps its id
    /// and a removed one's id is never reissued.
    names: Vec<(String, DeviceId)>,
    next: u32,
}

impl Default for DeviceTable {
    fn default() -> Self {
        Self {
            handles: Vec::new(),
            names: Vec::new(),
            next: FIRST_REAL_DEVICE,
        }
    }
}

impl DeviceTable {
    /// The id for a raw report's `hDevice`, or `None` for injected input (a
    /// null handle).
    ///
    /// `name` is asked only for a handle not seen before. A device whose path
    /// cannot be read still gets an id of its own, keyed by nothing, so it
    /// stays apart from every other device for as long as its handle lives.
    pub fn resolve(
        &mut self,
        handle: isize,
        name: impl FnOnce() -> Option<String>,
    ) -> Option<DeviceId> {
        if handle == 0 {
            return None;
        }
        if let Some(&(_, id)) = self.handles.iter().find(|&&(known, _)| known == handle) {
            return Some(id);
        }
        let named = name();
        let known = named
            .as_ref()
            .and_then(|name| self.names.iter().find(|(known, _)| known == name))
            .map(|&(_, id)| id);
        let id = known.unwrap_or_else(|| {
            let id = DeviceId(self.next);
            self.next += 1;
            if let Some(name) = named {
                self.names.push((name, id));
            }
            id
        });
        self.handles.push((handle, id));
        Some(id)
    }

    /// Forgets a handle whose device left. Its id stays reserved for the
    /// device's return.
    pub fn remove(&mut self, handle: isize) {
        self.handles.retain(|&(known, _)| known != handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYBOARD_A: DeviceId = DeviceId(10);
    const KEYBOARD_B: DeviceId = DeviceId(11);
    const MOUSE_A: DeviceId = DeviceId(12);

    #[test]
    fn a_message_takes_the_device_of_the_report_that_produced_it() {
        let mut attribution = Attribution::default();
        attribution.raw_key(0x1E, true, Some(KEYBOARD_A), 1_000);
        assert_eq!(
            attribution.key(0x1E, ButtonState::Pressed, 1_000),
            KEYBOARD_A
        );
        // Claimed once: the next matching message has no report left.
        assert_eq!(
            attribution.key(0x1E, ButtonState::Pressed, 1_001),
            KEYBOARD_DEVICE
        );
    }

    #[test]
    fn two_keyboards_typing_in_one_pump_keep_their_own_keys() {
        // Posted messages come out before input messages, so both reports are
        // recorded before either key message is translated.
        let mut attribution = Attribution::default();
        attribution.raw_key(0x1E, true, Some(KEYBOARD_A), 1_000);
        attribution.raw_key(0x1F, true, Some(KEYBOARD_B), 1_001);
        assert_eq!(
            attribution.key(0x1E, ButtonState::Pressed, 1_000),
            KEYBOARD_A
        );
        assert_eq!(
            attribution.key(0x1F, ButtonState::Pressed, 1_001),
            KEYBOARD_B
        );
    }

    #[test]
    fn a_report_whose_message_never_came_is_passed_over_by_content() {
        // Keyboard A's Tab was consumed by the system (Alt+Tab), so it has a
        // report and no message; keyboard B's A follows inside the window. The
        // oldest report is A's, and it is not the one B's key produced.
        let mut attribution = Attribution::default();
        attribution.raw_key(0x0F, true, Some(KEYBOARD_A), 1_000);
        attribution.raw_key(0x1E, true, Some(KEYBOARD_B), 1_010);
        assert_eq!(
            attribution.key(0x1E, ButtonState::Pressed, 1_010),
            KEYBOARD_B
        );
        // And a click is not claimed by a key report or the other way round.
        attribution.raw_mouse(value::RI_MOUSE_LEFT_BUTTON_DOWN, Some(MOUSE_A), 1_020);
        assert_eq!(
            attribution.key(0x0F, ButtonState::Pressed, 1_020),
            KEYBOARD_A
        );
        assert_eq!(
            attribution.button(PointerButton::Left, ButtonState::Pressed, 1_020),
            MOUSE_A
        );
    }

    #[test]
    fn the_same_key_on_two_keyboards_is_claimed_in_order() {
        let mut attribution = Attribution::default();
        attribution.raw_key(0x1E, true, Some(KEYBOARD_A), 1_000);
        attribution.raw_key(0x1E, true, Some(KEYBOARD_B), 1_002);
        assert_eq!(
            attribution.key(0x1E, ButtonState::Pressed, 1_000),
            KEYBOARD_A
        );
        assert_eq!(
            attribution.key(0x1E, ButtonState::Pressed, 1_002),
            KEYBOARD_B
        );
    }

    #[test]
    fn a_press_does_not_claim_a_release_and_the_prefix_is_part_of_the_code() {
        let mut attribution = Attribution::default();
        attribution.raw_key(0xE048, false, Some(KEYBOARD_A), 1_000);
        // Numpad 8 shares ArrowUp's low byte; only the E0 prefix differs.
        assert_eq!(
            attribution.key(0x48, ButtonState::Released, 1_000),
            KEYBOARD_DEVICE
        );
        assert_eq!(
            attribution.key(0xE048, ButtonState::Pressed, 1_000),
            KEYBOARD_DEVICE
        );
        assert_eq!(
            attribution.key(0xE048, ButtonState::Released, 1_000),
            KEYBOARD_A
        );
    }

    #[test]
    fn a_report_nothing_claimed_ages_out_instead_of_being_given_to_a_later_key() {
        // Alt+Tab: the system consumed the key, so its message never arrives.
        let mut attribution = Attribution::default();
        attribution.raw_key(0x0F, true, Some(KEYBOARD_A), 1_000);
        let later = 1_000 + MATCH_WINDOW_MS + 1;
        assert_eq!(
            attribution.key(0x0F, ButtonState::Pressed, later),
            KEYBOARD_DEVICE
        );
        assert!(attribution.pending.is_empty(), "{:?}", attribution.pending);
    }

    #[test]
    fn matching_survives_the_tick_count_wrapping() {
        let mut attribution = Attribution::default();
        attribution.raw_key(0x1E, true, Some(KEYBOARD_A), u32::MAX - 2);
        assert_eq!(attribution.key(0x1E, ButtonState::Pressed, 3), KEYBOARD_A);
    }

    #[test]
    fn injected_input_carries_the_per_kind_device() {
        let mut attribution = Attribution::default();
        attribution.raw_key(0x1E, true, None, 1_000);
        assert_eq!(
            attribution.key(0x1E, ButtonState::Pressed, 1_000),
            KEYBOARD_DEVICE
        );
        attribution.raw_mouse(value::RI_MOUSE_LEFT_BUTTON_DOWN, None, 1_000);
        assert_eq!(
            attribution.button(PointerButton::Left, ButtonState::Pressed, 1_000),
            POINTER_DEVICE
        );
    }

    #[test]
    fn a_mouse_report_names_the_pointer_and_every_edge_it_carries() {
        let mut attribution = Attribution::default();
        let flags = value::RI_MOUSE_LEFT_BUTTON_DOWN
            | (value::RI_MOUSE_RIGHT_BUTTON_DOWN << 1)
            | value::RI_MOUSE_BUTTON_5_DOWN
            | value::RI_MOUSE_WHEEL;
        attribution.raw_mouse(flags, Some(MOUSE_A), 2_000);
        assert_eq!(attribution.pointer(), MOUSE_A);
        assert_eq!(
            attribution.button(PointerButton::Left, ButtonState::Pressed, 2_000),
            MOUSE_A
        );
        assert_eq!(
            attribution.button(PointerButton::Right, ButtonState::Released, 2_000),
            MOUSE_A
        );
        assert_eq!(
            attribution.button(PointerButton::Forward, ButtonState::Pressed, 2_000),
            MOUSE_A
        );
        assert_eq!(attribution.wheel(false, 2_000), MOUSE_A);
        assert!(attribution.pending.is_empty(), "{:?}", attribution.pending);
        // A button message with no report falls back to the pointer's device,
        // not to the constant: it is the same mouse's click.
        assert_eq!(
            attribution.button(PointerButton::Middle, ButtonState::Pressed, 2_001),
            MOUSE_A
        );
    }

    #[test]
    fn the_raw_scancode_folds_the_prefix_as_the_message_does() {
        assert_eq!(raw_scancode(0x1E, 0), Some(0x1E));
        assert_eq!(raw_scancode(0x48, value::RI_KEY_E0), Some(0xE048));
        assert_eq!(
            raw_scancode(0x1D, value::RI_KEY_E0 | value::RI_KEY_BREAK),
            Some(0xE01D)
        );
        assert_eq!(raw_scancode(0x1D, value::RI_KEY_E1), None);
    }

    #[test]
    fn a_device_keeps_its_id_across_a_replug_and_ids_are_never_reissued() {
        let mut table = DeviceTable::default();
        let path = || Some("\\\\?\\HID#VID_3151".to_owned());
        assert_eq!(table.resolve(0, path), None, "injected input has no device");
        let first = table.resolve(0x100, path).expect("a real handle");
        assert!(first.0 >= FIRST_REAL_DEVICE, "{first:?}");
        assert_eq!(table.resolve(0x100, || panic!("cached")), Some(first));

        let other = table
            .resolve(0x200, || Some("\\\\?\\HID#VID_3710".to_owned()))
            .expect("a second device");
        assert_ne!(other, first);

        // Unplugged, then back under a new handle.
        table.remove(0x100);
        assert_eq!(table.resolve(0x300, path), Some(first));
        // And a new device that happens to get the old handle value is not
        // mistaken for the old one.
        let newcomer = table
            .resolve(0x100, || Some("\\\\?\\HID#VID_9999".to_owned()))
            .expect("a third device");
        assert_ne!(newcomer, first);
        assert_ne!(newcomer, other);
    }

    #[test]
    fn a_device_with_no_readable_path_still_gets_an_id_of_its_own() {
        let mut table = DeviceTable::default();
        let a = table.resolve(0x100, || None).expect("an id");
        let b = table.resolve(0x200, || None).expect("an id");
        assert_ne!(a, b);
    }
}
