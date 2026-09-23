//! Hand-written FFI to XInput, and the two structures `XInputGetState` fills.
//!
//! **Loaded at runtime, not linked** — the opposite of the Win32 shell's
//! `#[link]` decision, for the reason that module's docs give for when the
//! opposite would be right: a library that can be absent. `xinput1_4.dll` ships
//! with Windows 8 and later, but a stripped Windows (Server Core, some N and
//! container images) can lack it, and a `#[link]` would stop the process in the
//! loader before a game could fall back to keyboard and mouse.
//! `xinput9_1_0.dll` is the older redistributable name, tried second.

#![allow(non_snake_case)]

/// `XINPUT_GAMEPAD`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct XInputGamepad {
    /// `wButtons`: a bitmask of the `XINPUT_GAMEPAD_*` button constants.
    pub(crate) buttons: u16,
    /// `bLeftTrigger`: 0 at rest, 255 fully pulled.
    pub(crate) left_trigger: u8,
    /// `bRightTrigger`.
    pub(crate) right_trigger: u8,
    /// `sThumbLX`: −32768…32767, positive right.
    pub(crate) thumb_lx: i16,
    /// `sThumbLY`: −32768…32767, **positive up** — already the seam's sign.
    pub(crate) thumb_ly: i16,
    /// `sThumbRX`.
    pub(crate) thumb_rx: i16,
    /// `sThumbRY`, positive up.
    pub(crate) thumb_ry: i16,
}

/// `XINPUT_STATE`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct XInputState {
    /// `dwPacketNumber`: changes whenever the state does. Not read: the poller
    /// compares mapped snapshots, which is the same answer without trusting a
    /// counter the backend cannot test.
    pub(crate) packet_number: u32,
    /// `Gamepad`.
    pub(crate) gamepad: XInputGamepad,
}

/// `XUSER_MAX_COUNT`: XInput's user slots, 0 through 3.
pub(crate) const XUSER_MAX_COUNT: u32 = 4;

/// `ERROR_SUCCESS`: the slot has a controller, and the state is filled in.
#[cfg(windows)]
pub(crate) const ERROR_SUCCESS: u32 = 0;
/// `ERROR_DEVICE_NOT_CONNECTED`: the slot is empty. Not a failure.
pub(crate) const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;

// The `XINPUT_GAMEPAD_*` button bits. 0x0400 is the Guide button, which only
// the undocumented ordinal-100 `XInputGetStateEx` reports; `XInputGetState`
// never sets it, so it is not declared.
pub(crate) const XINPUT_GAMEPAD_DPAD_UP: u16 = 0x0001;
pub(crate) const XINPUT_GAMEPAD_DPAD_DOWN: u16 = 0x0002;
pub(crate) const XINPUT_GAMEPAD_DPAD_LEFT: u16 = 0x0004;
pub(crate) const XINPUT_GAMEPAD_DPAD_RIGHT: u16 = 0x0008;
pub(crate) const XINPUT_GAMEPAD_START: u16 = 0x0010;
pub(crate) const XINPUT_GAMEPAD_BACK: u16 = 0x0020;
pub(crate) const XINPUT_GAMEPAD_LEFT_THUMB: u16 = 0x0040;
pub(crate) const XINPUT_GAMEPAD_RIGHT_THUMB: u16 = 0x0080;
pub(crate) const XINPUT_GAMEPAD_LEFT_SHOULDER: u16 = 0x0100;
pub(crate) const XINPUT_GAMEPAD_RIGHT_SHOULDER: u16 = 0x0200;
pub(crate) const XINPUT_GAMEPAD_A: u16 = 0x1000;
pub(crate) const XINPUT_GAMEPAD_B: u16 = 0x2000;
pub(crate) const XINPUT_GAMEPAD_X: u16 = 0x4000;
pub(crate) const XINPUT_GAMEPAD_Y: u16 = 0x8000;

/// `XInputGetState(DWORD dwUserIndex, XINPUT_STATE *pState) -> DWORD`.
#[cfg(windows)]
pub(crate) type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XInputState) -> u32;

/// `HMODULE`.
#[cfg(windows)]
pub(crate) type Module = *mut core::ffi::c_void;

// The loader half of `kernel32`: all this backend needs to find XInput.
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn LoadLibraryW(name: *const u16) -> Module;
    pub(crate) fn GetProcAddress(module: Module, name: *const u8) -> *mut core::ffi::c_void;
    pub(crate) fn FreeLibrary(module: Module) -> i32;
    pub(crate) fn GetLastError() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The width of the field `select` picks out, read from its type alone.
    fn field_size<T, F>(_select: fn(&T) -> &F) -> usize {
        size_of::<F>()
    }

    /// Asserts a structure's size and every field's offset and width — the
    /// Win32 shell's `assert_layout!` (`crcbl-shell`'s `win32::ffi` tests),
    /// which a test in another crate cannot reach. The pattern names every
    /// field with no `..`, so a field added without a row fails to compile.
    macro_rules! assert_layout {
        ($ty:ident, $size:literal, { $($field:ident: $offset:literal, $width:literal;)+ }) => {{
            let _every_field_has_a_row: fn($ty) = |value| {
                let $ty { $($field: _),+ } = value;
            };
            assert_eq!(size_of::<$ty>(), $size, concat!("size of ", stringify!($ty)));
            $(
                assert_eq!(
                    core::mem::offset_of!($ty, $field),
                    $offset,
                    concat!("offset of ", stringify!($ty), "::", stringify!($field))
                );
                assert_eq!(
                    field_size(|value: &$ty| &value.$field),
                    $width,
                    concat!("width of ", stringify!($ty), "::", stringify!($field))
                );
            )+
        }};
    }

    /// The layouts against `Xinput.h`. Every number is the SDK's own
    /// `sizeof`/`offsetof`, printed by a C program built with MSVC 19.44
    /// (`_MSC_FULL_VER` 194435229) against Windows SDK 10.0.26100.0 for x64 —
    /// the same toolchain the Win32 shell's table was printed with. Every field
    /// is fixed-width, so these hold on every target the tests run on, not only
    /// Windows.
    #[test]
    fn the_structures_match_the_c_layout() {
        assert_layout!(XInputGamepad, 12, {
            buttons: 0, 2;
            left_trigger: 2, 1;
            right_trigger: 3, 1;
            thumb_lx: 4, 2;
            thumb_ly: 6, 2;
            thumb_rx: 8, 2;
            thumb_ry: 10, 2;
        });
        assert_layout!(XInputState, 16, {
            packet_number: 0, 4;
            gamepad: 4, 12;
        });
    }
}
