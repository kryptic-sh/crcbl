//! Which kind of device spoke last — what a UI's mixed-input rule reads to
//! decide between showing hover and showing focus, and what
//! [`ActionMap::hint`] reads to show `Space` or a pad button.
//!
//! **Only activity counts.** A key or button press, pointer movement, a wheel
//! turn, an on-screen control pressed or deflected, and a pad button pressed or
//! a pad stick or trigger pushed out past
//! [`PAD_ACTIVITY_THRESHOLD`](crate::PAD_ACTIVITY_THRESHOLD) each set it; a
//! release does not, since letting go of a key after reaching for the mouse is not the
//! keyboard speaking, and neither does a zero delta. It is tracked from every
//! event the map receives, whether or not any binding reads that input.

use super::{ActionMap, Binding, PadKind};

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
        self.devices.first().copied()
    }

    /// Whether [`ActionMap::last_device`] changed since the last
    /// [`ActionMap::begin_tick`] — the edge a game swapping its control
    /// prompts reads, as it reads [`ActionMap::just_pressed`] for a button.
    ///
    /// Set only when a different device takes the front: the keyboard
    /// speaking again while it is already the last device changes nothing,
    /// and neither does a device moving up behind it. What counts as speaking
    /// is `device.rs`'s rule, so a pad stick drifting inside
    /// [`PAD_ACTIVITY_THRESHOLD`](crate::PAD_ACTIVITY_THRESHOLD) never raises
    /// it.
    #[must_use]
    pub const fn last_device_changed(&self) -> bool {
        self.device_changed
    }

    /// The family of the pad that last spoke, by the same rule, or `None`
    /// before any pad did — what [`ActionMap::hint`] names a pad button after.
    ///
    /// Kept apart from [`ActionMap::last_device`] because it outlives it: a
    /// player who put the pad down for the keyboard and picks it up again is
    /// still holding the pad they were.
    #[must_use]
    pub fn last_pad_kind(&self) -> Option<PadKind> {
        self.last_pad_kind
    }

    /// Records that `device` spoke: it moves to the front of the recency
    /// list, and the rest keep their order.
    pub(crate) fn spoke(&mut self, device: Device) {
        if self.devices.first() != Some(&device) {
            self.device_changed = true;
        }
        if let Some(index) = self.devices.iter().position(|&seen| seen == device) {
            self.devices.remove(index);
        }
        self.devices.insert(0, device);
    }
}

impl Binding {
    /// The kind of device this binding listens to — which list of a binding
    /// asset it is written in (`binding_asset.rs`), and what
    /// [`ActionMap::hint`] matches against [`ActionMap::last_device`].
    ///
    /// A binding that reads two devices belongs to the one its value comes
    /// from: a [`Binding::ScrollChord`] is the wheel with a key held, so it is
    /// [`Device::Pointer`]'s. [`Binding::PointerPosition`] is the pointer's
    /// too, though a phone's primary contact drives it — the platform hands
    /// that contact over as a pointer.
    #[must_use]
    pub const fn device(&self) -> Device {
        match self {
            Self::Key(_) | Self::Chord { .. } | Self::KeyAxis { .. } | Self::Wasd { .. } => {
                Device::Keyboard
            }
            Self::MouseButton(_)
            | Self::ButtonChord { .. }
            | Self::MouseMotion
            | Self::MouseScroll
            | Self::ScrollChord { .. }
            | Self::PointerPosition { .. } => Device::Pointer,
            Self::Virtual(_) => Device::Touch,
            Self::PadButton(_)
            | Self::PadChord { .. }
            | Self::PadDpad
            | Self::PadStick { .. }
            | Self::PadTrigger { .. } => Device::Gamepad,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PadButton, PointerAxis};
    use crcbl_core::input::{KeyCode, PointerButton};

    /// A binding that reads two devices belongs to the one its value comes
    /// from, as [`Binding::device`] documents.
    #[test]
    fn a_binding_belongs_to_the_device_its_value_comes_from() {
        for (binding, device) in [
            (
                Binding::Chord {
                    modifier: crate::Modifier::Shift,
                    key: KeyCode::KeyR,
                },
                Device::Keyboard,
            ),
            (
                Binding::ScrollChord {
                    held: KeyCode::ControlLeft,
                },
                Device::Pointer,
            ),
            (
                Binding::ButtonChord {
                    modifier: crate::Modifier::Alt,
                    button: PointerButton::Right,
                },
                Device::Pointer,
            ),
            (
                Binding::PointerPosition {
                    axis: PointerAxis::Y,
                },
                Device::Pointer,
            ),
            (Binding::Virtual("stick".to_owned()), Device::Touch),
            (
                Binding::PadChord {
                    modifier: PadButton::LeftShoulder,
                    button: PadButton::South,
                },
                Device::Gamepad,
            ),
        ] {
            assert_eq!(binding.device(), device, "{binding}");
        }
    }

    /// **The change edge rises when a different device takes over on a real
    /// press, and not on a pad stick drifting inside the activity threshold**,
    /// nor on the same device speaking again; [`ActionMap::begin_tick`] clears
    /// it.
    #[test]
    fn the_last_device_changes_on_a_real_press_and_not_on_drift() {
        use crate::{GamepadEvent, GamepadId, GamepadSnapshot, PAD_ACTIVITY_THRESHOLD, PadAxis};

        let pad = |map: &mut ActionMap, edit: &dyn Fn(&mut GamepadSnapshot)| {
            let mut snapshot = GamepadSnapshot::neutral(PadKind::Xbox);
            edit(&mut snapshot);
            map.gamepad_event(&GamepadEvent::State {
                id: GamepadId(4),
                snapshot,
            });
        };
        let mut map = ActionMap::new();
        assert!(!map.last_device_changed());

        map.key_event(KeyCode::KeyQ, true);
        assert!(map.last_device_changed(), "the first device to speak");
        map.begin_tick(0.0);
        assert!(!map.last_device_changed(), "begin_tick clears the edge");

        let drift = PAD_ACTIVITY_THRESHOLD * 0.9;
        pad(&mut map, &|pad| pad.axes[PadAxis::LeftX as usize] = drift);
        pad(&mut map, &|pad| pad.axes[PadAxis::LeftY as usize] = -drift);
        assert_eq!(
            map.last_device(),
            Some(Device::Keyboard),
            "drift is not a push"
        );
        assert!(!map.last_device_changed(), "and raises nothing");

        map.key_event(KeyCode::KeyE, true);
        assert!(
            !map.last_device_changed(),
            "the keyboard speaking again changes nothing"
        );

        pad(&mut map, &|pad| pad.buttons.insert(PadButton::South));
        assert_eq!(map.last_device(), Some(Device::Gamepad));
        assert!(map.last_device_changed(), "a pad press takes over");
    }

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
