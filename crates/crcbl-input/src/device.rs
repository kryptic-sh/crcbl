//! Which kind of device spoke last — what a UI's mixed-input rule reads to
//! decide between showing hover and showing focus, and what a glyph hint will
//! read to show `Space` or a pad button.
//!
//! **Only activity counts.** A key or button press, pointer movement, a wheel
//! turn, an on-screen control pressed or deflected, and a pad button pressed or
//! a pad stick or trigger pushed out past
//! [`PAD_ACTIVITY_THRESHOLD`](crate::PAD_ACTIVITY_THRESHOLD) each set it; a
//! release does not, since letting go of a key after reaching for the mouse is not the
//! keyboard speaking, and neither does a zero delta. It is tracked from every
//! event the map receives, whether or not any binding reads that input.

use super::ActionMap;

/// A kind of input device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Device {
    /// A key.
    Keyboard,
    /// A mouse, or a platform's emulated pointer — which is how a phone's
    /// primary contact arrives.
    Pointer,
    /// An on-screen control, reported through [`ActionMap::virtual_button`] or
    /// [`ActionMap::virtual_stick`].
    Touch,
    /// A gamepad. **Every backend reports through
    /// [`ActionMap::gamepad_event`]**, so which one spoke — XInput, evdev,
    /// Steam Input — is not something this can tell apart, by design.
    Gamepad,
}

impl ActionMap {
    /// The kind of device that last spoke, or `None` before any did.
    #[must_use]
    pub fn last_device(&self) -> Option<Device> {
        self.last_device
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::input::{KeyCode, PointerButton};

    /// **Each device takes over when it speaks**, whether or not anything is
    /// bound to what it said — and a release or a still pointer does not.
    #[test]
    fn the_last_device_follows_activity_and_not_releases() {
        let mut map = ActionMap::new();
        assert_eq!(map.last_device(), None);

        map.key_event(KeyCode::KeyQ, true);
        assert_eq!(map.last_device(), Some(Device::Keyboard), "nothing binds Q");

        map.mouse_motion(0.0, 0.0);
        map.mouse_scroll(0.0, 0.0);
        assert_eq!(
            map.last_device(),
            Some(Device::Keyboard),
            "a zero delta is not movement"
        );

        map.mouse_motion(3.0, 0.0);
        assert_eq!(map.last_device(), Some(Device::Pointer));

        map.key_event(KeyCode::KeyQ, false);
        assert_eq!(
            map.last_device(),
            Some(Device::Pointer),
            "letting go of a key is not the keyboard speaking",
        );

        map.virtual_stick("stick", 0.0, 0.0);
        assert_eq!(
            map.last_device(),
            Some(Device::Pointer),
            "a centred stick said nothing"
        );
        map.virtual_stick("stick", 0.5, 0.0);
        assert_eq!(map.last_device(), Some(Device::Touch));

        map.pointer_position(0.2, 0.3);
        assert_eq!(map.last_device(), Some(Device::Pointer));
        map.key_event(KeyCode::Enter, true);
        map.pointer_position(0.2, 0.3);
        assert_eq!(
            map.last_device(),
            Some(Device::Keyboard),
            "a pointer re-reporting where it already is has not moved",
        );

        map.virtual_button("btn", true);
        assert_eq!(map.last_device(), Some(Device::Touch));
        map.mouse_button(PointerButton::Left, true);
        assert_eq!(map.last_device(), Some(Device::Pointer));
        map.mouse_scroll(0.0, 1.0);
        map.key_event(KeyCode::Enter, true);
        assert_eq!(map.last_device(), Some(Device::Keyboard));
        map.mouse_scroll(0.0, 1.0);
        assert_eq!(map.last_device(), Some(Device::Pointer));
    }
}
