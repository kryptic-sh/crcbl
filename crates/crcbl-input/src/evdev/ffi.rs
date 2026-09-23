//! The kernel's evdev ABI: the structures `read` and the `EVIOCG*` ioctls fill,
//! the event codes this backend reads, and the ioctl request numbers.
//!
//! Declared by hand from the UAPI headers `linux/input.h` and
//! `linux/input-event-codes.h`, for the reason `crcbl-shell`'s Wayland FFI
//! gives for declaring its few libc functions: `libc` would be a dependency in
//! the engine's graph for one `ioctl`. Opening and reading a node go through
//! `std::fs::File`, which opens with `O_CLOEXEC` on Linux already.
//!
//! Everything but the `ioctl` declaration and `O_NONBLOCK` is plain data and
//! arithmetic, compiled into every target's tests so the layouts, the request
//! numbers and the event decoding are checked off Linux too.

use std::io;

/// `__kernel_ulong_t`, the kernel's `unsigned long`: pointer-width on every
/// Linux ABI this backend builds for (LP64 and ILP32). Defined from the
/// pointer width rather than as `core::ffi::c_ulong`, which is 32 bits on
/// 64-bit Windows, so the tests that run there check the Linux layout.
#[cfg(target_pointer_width = "64")]
pub(crate) type KernelUlong = u64;
/// `__kernel_ulong_t` on a 32-bit target — see the 64-bit definition.
#[cfg(target_pointer_width = "32")]
pub(crate) type KernelUlong = u32;

// The ABIs whose `O_NONBLOCK` and `_IOC` encoding are the ones written here:
// the asm-generic values, which x86 and ARM share. Alpha, MIPS, PowerPC, SPARC
// and PA-RISC differ in one or both, and x32 has a 64-bit `__kernel_ulong_t`
// behind 32-bit pointers, so each is refused rather than built wrong.
#[cfg(all(
    target_os = "linux",
    not(any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64",
    ))
))]
compile_error!(
    "the evdev backend's O_NONBLOCK and ioctl encoding are unverified for this architecture"
);
#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    target_pointer_width = "32"
))]
compile_error!("the evdev backend does not support x32, whose __kernel_ulong_t is 64-bit");

/// `struct input_event`, what one `read` of an evdev node returns a run of.
///
/// The two time words are `struct timeval` on a 64-bit target and the
/// `__sec`/`__usec` pair of `__kernel_ulong_t` on a 32-bit one — the header
/// switches to the pair so the structure keeps its size when userspace's
/// `time_t` goes 64-bit. Either way they are two `unsigned long`s, which is
/// what [`KernelUlong`] is. The backend never reads them: a snapshot belongs
/// to the poll that reports it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputEvent {
    /// `time.tv_sec` / `__sec`.
    pub(crate) sec: KernelUlong,
    /// `time.tv_usec` / `__usec`.
    pub(crate) usec: KernelUlong,
    /// `type`: `EV_SYN`, `EV_KEY`, `EV_ABS`, …
    pub(crate) kind: u16,
    /// `code`: which key or axis.
    pub(crate) code: u16,
    /// `value`: 1 pressed, 0 released, 2 autorepeat for a key; the position
    /// for an axis.
    pub(crate) value: i32,
}

/// Bytes in one [`InputEvent`].
pub(crate) const EVENT_SIZE: usize = size_of::<InputEvent>();

impl InputEvent {
    /// An event with no timestamp, as a test's script writes one.
    #[cfg(test)]
    pub(crate) const fn new(kind: u16, code: u16, value: i32) -> Self {
        Self {
            sec: 0,
            usec: 0,
            kind,
            code,
            value,
        }
    }

    /// One event from the bytes `read` put down, native-endian, at the
    /// offsets the structure itself has.
    fn from_bytes(bytes: &[u8; EVENT_SIZE]) -> Self {
        fn field<const N: usize>(bytes: &[u8; EVENT_SIZE], at: usize) -> [u8; N] {
            *bytes[at..]
                .first_chunk()
                .expect("the layout test pins every field inside the structure")
        }
        Self {
            sec: KernelUlong::from_ne_bytes(field(bytes, core::mem::offset_of!(Self, sec))),
            usec: KernelUlong::from_ne_bytes(field(bytes, core::mem::offset_of!(Self, usec))),
            kind: u16::from_ne_bytes(field(bytes, core::mem::offset_of!(Self, kind))),
            code: u16::from_ne_bytes(field(bytes, core::mem::offset_of!(Self, code))),
            value: i32::from_ne_bytes(field(bytes, core::mem::offset_of!(Self, value))),
        }
    }
}

/// The events in what one `read` returned.
///
/// # Errors
/// [`io::ErrorKind::InvalidData`] if `bytes` is not a whole number of events.
/// The kernel only ever copies whole events, so this is a broken node, not a
/// short read to wait out.
pub(crate) fn decode_events(bytes: &[u8]) -> io::Result<impl Iterator<Item = InputEvent>> {
    let (events, rest) = bytes.as_chunks::<EVENT_SIZE>();
    if rest.is_empty() {
        Ok(events.iter().map(InputEvent::from_bytes))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "an evdev read returned {} bytes, not a multiple of {EVENT_SIZE}",
                bytes.len()
            ),
        ))
    }
}

/// `struct input_absinfo`, what `EVIOCGABS` fills for one axis.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AbsInfo {
    /// The axis's position now.
    pub(crate) value: i32,
    /// The least value it reports.
    pub(crate) minimum: i32,
    /// The greatest value it reports.
    pub(crate) maximum: i32,
    /// The noise the kernel already filters out. Not read.
    pub(crate) fuzz: i32,
    /// The driver's suggested dead zone around the centre. Not applied: the
    /// seam's axes are raw, and the dead zone is a binding's (`gamepad.rs`).
    pub(crate) flat: i32,
    /// Units per millimetre or per radian. Not read.
    pub(crate) resolution: i32,
}

/// `struct input_id`, what `EVIOCGID` fills.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputId {
    /// `bustype`: `BUS_USB`, `BUS_BLUETOOTH`, … Not read.
    pub(crate) bustype: u16,
    /// The USB vendor id.
    pub(crate) vendor: u16,
    /// The USB product id.
    pub(crate) product: u16,
    /// The device's version. Not read.
    pub(crate) version: u16,
}

// Event types.
pub(crate) const EV_SYN: u16 = 0x00;
pub(crate) const EV_KEY: u16 = 0x01;
pub(crate) const EV_ABS: u16 = 0x03;

// `EV_SYN` codes.
pub(crate) const SYN_REPORT: u16 = 0;
pub(crate) const SYN_DROPPED: u16 = 3;

// `EV_KEY` codes. `BTN_NORTH` is also spelled `BTN_X`, and `BTN_WEST` `BTN_Y`,
// which is the source of the face-button split `map.rs` describes.
pub(crate) const BTN_GAMEPAD: u16 = 0x130;
pub(crate) const BTN_SOUTH: u16 = 0x130;
pub(crate) const BTN_EAST: u16 = 0x131;
pub(crate) const BTN_NORTH: u16 = 0x133;
pub(crate) const BTN_WEST: u16 = 0x134;
pub(crate) const BTN_TL: u16 = 0x136;
pub(crate) const BTN_TR: u16 = 0x137;
pub(crate) const BTN_TL2: u16 = 0x138;
pub(crate) const BTN_TR2: u16 = 0x139;
pub(crate) const BTN_SELECT: u16 = 0x13a;
pub(crate) const BTN_START: u16 = 0x13b;
pub(crate) const BTN_MODE: u16 = 0x13c;
pub(crate) const BTN_THUMBL: u16 = 0x13d;
pub(crate) const BTN_THUMBR: u16 = 0x13e;
pub(crate) const BTN_DPAD_UP: u16 = 0x220;
pub(crate) const BTN_DPAD_DOWN: u16 = 0x221;
pub(crate) const BTN_DPAD_LEFT: u16 = 0x222;
pub(crate) const BTN_DPAD_RIGHT: u16 = 0x223;
/// `KEY_MAX + 1`: how many key codes a key bitmap covers.
pub(crate) const KEY_CNT: usize = 0x300;

// `EV_ABS` codes.
pub(crate) const ABS_X: u16 = 0x00;
pub(crate) const ABS_Y: u16 = 0x01;
pub(crate) const ABS_Z: u16 = 0x02;
pub(crate) const ABS_RX: u16 = 0x03;
pub(crate) const ABS_RY: u16 = 0x04;
pub(crate) const ABS_RZ: u16 = 0x05;
pub(crate) const ABS_GAS: u16 = 0x09;
pub(crate) const ABS_BRAKE: u16 = 0x0a;
pub(crate) const ABS_HAT0X: u16 = 0x10;
pub(crate) const ABS_HAT0Y: u16 = 0x11;
pub(crate) const ABS_HAT2X: u16 = 0x14;
pub(crate) const ABS_HAT2Y: u16 = 0x15;
/// `ABS_MAX + 1`: how many axis codes an axis bitmap covers.
pub(crate) const ABS_CNT: usize = 0x40;

/// Bits in one bitmap word.
const WORD_BITS: usize = KernelUlong::BITS as usize;

/// A kernel bitmap, as `EVIOCGBIT` and `EVIOCGKEY` fill it: an array of
/// `unsigned long`, bit *n* being bit `n % WORD_BITS` of word `n / WORD_BITS`
/// — read by word, so it holds on either byte order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bits<const WORDS: usize>(pub(crate) [KernelUlong; WORDS]);

impl<const WORDS: usize> Bits<WORDS> {
    /// No bit set.
    pub(crate) const EMPTY: Self = Self([0; WORDS]);

    /// Whether bit `code` is set; a code past the end is not.
    pub(crate) fn has(&self, code: u16) -> bool {
        let code = usize::from(code);
        self.0
            .get(code / WORD_BITS)
            .is_some_and(|word| word >> (code % WORD_BITS) & 1 != 0)
    }

    /// Sets or clears bit `code`; a code past the end is ignored, as the
    /// kernel never sends one.
    pub(crate) fn set(&mut self, code: u16, on: bool) {
        let code = usize::from(code);
        if let Some(word) = self.0.get_mut(code / WORD_BITS) {
            let bit = 1 << (code % WORD_BITS);
            if on {
                *word |= bit;
            } else {
                *word &= !bit;
            }
        }
    }
}

/// Every key code's bit.
pub(crate) type KeyBits = Bits<{ KEY_CNT.div_ceil(WORD_BITS) }>;
/// Every axis code's bit.
pub(crate) type AbsBits = Bits<{ ABS_CNT.div_ceil(WORD_BITS) }>;

/// `_IOC_READ`, in the asm-generic encoding the architectures above share.
const IOC_READ: u32 = 2;
/// `_IOC_SIZEBITS`.
const IOC_SIZE_BITS: u32 = 14;

/// `_IOC(_IOC_READ, 'E', nr, size)`: an evdev request that has the kernel
/// write `size` bytes back.
///
/// # Panics
/// If `size` does not fit the encoding's size field. Every size passed is a
/// structure's, far under it.
pub(crate) const fn evioc_read(nr: u32, size: usize) -> u32 {
    assert!(
        size < 1 << IOC_SIZE_BITS,
        "an ioctl's size field is 14 bits"
    );
    (IOC_READ << 30) | ((size as u32) << 16) | ((b'E' as u32) << 8) | nr
}

/// `EVIOCGID`'s request number: the device's [`InputId`].
pub(crate) const EVIOCGID_NR: u32 = 0x02;
/// `EVIOCGKEY`'s: the keys held now, as a [`KeyBits`].
pub(crate) const EVIOCGKEY_NR: u32 = 0x18;
/// `EVIOCGBIT(ev)`'s: the codes of event type `ev` the device can send.
pub(crate) const fn eviocgbit_nr(ev: u16) -> u32 {
    0x20 + ev as u32
}
/// `EVIOCGABS(abs)`'s: axis `abs`'s [`AbsInfo`].
pub(crate) const fn eviocgabs_nr(abs: u16) -> u32 {
    0x40 + abs as u32
}

/// `O_NONBLOCK`, asm-generic's value, which x86 and ARM share.
#[cfg(target_os = "linux")]
pub(crate) const O_NONBLOCK: i32 = 0o4000;

/// `ENODEV`: the device behind an open node is gone. The same number on
/// every Linux architecture (`asm-generic/errno-base.h`).
pub(crate) const ENODEV: i32 = 19;

/// The type of `ioctl`'s request argument: `unsigned long` in glibc, `int` in
/// musl.
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
pub(crate) type IoctlRequest = core::ffi::c_ulong;
/// The type of `ioctl`'s request argument in musl.
#[cfg(all(target_os = "linux", target_env = "musl"))]
pub(crate) type IoctlRequest = core::ffi::c_int;

/// A request number as the C library's `ioctl` takes it.
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
pub(crate) fn ioctl_request(request: u32) -> IoctlRequest {
    IoctlRequest::from(request)
}
/// A request number as musl's `ioctl` takes it: the same 32 bits, as `int`.
#[cfg(all(target_os = "linux", target_env = "musl"))]
pub(crate) fn ioctl_request(request: u32) -> IoctlRequest {
    request.cast_signed()
}

// From the C library, which `std` already links on Linux.
#[cfg(target_os = "linux")]
unsafe extern "C" {
    pub(crate) fn ioctl(fd: core::ffi::c_int, request: IoctlRequest, ...) -> core::ffi::c_int;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi_layout::assert_layout;

    /// The layouts against the UAPI headers, worked by hand from their field
    /// types: no Linux C compiler is on the machine these were written on, so
    /// no `offsetof` was printed. `input_event` is two `unsigned long`s then
    /// `__u16, __u16, __s32` — 24 bytes on a 64-bit target (x86_64, aarch64)
    /// and 16 on a 32-bit one — and the other two are fixed-width fields
    /// with no padding.
    #[test]
    fn the_structures_match_the_c_layout() {
        let word = size_of::<usize>();
        assert_layout!(InputEvent, 2 * word + 8, {
            sec: 0, word;
            usec: word, word;
            kind: 2 * word, 2;
            code: 2 * word + 2, 2;
            value: 2 * word + 4, 4;
        });
        #[cfg(target_pointer_width = "64")]
        assert_eq!(EVENT_SIZE, 24, "x86_64 and aarch64");
        #[cfg(target_pointer_width = "32")]
        assert_eq!(EVENT_SIZE, 16, "32-bit, with either time_t");
        assert_layout!(AbsInfo, 24, {
            value: 0, 4;
            minimum: 4, 4;
            maximum: 8, 4;
            fuzz: 12, 4;
            flat: 16, 4;
            resolution: 20, 4;
        });
        assert_layout!(InputId, 8, {
            bustype: 0, 2;
            vendor: 2, 2;
            product: 4, 2;
            version: 6, 2;
        });
        assert_eq!(size_of::<KeyBits>(), KEY_CNT / 8, "96 bytes of key bits");
        assert_eq!(size_of::<AbsBits>(), ABS_CNT / 8, "8 bytes of axis bits");
    }

    /// The request numbers against ones worked by hand from `_IOC`'s layout —
    /// `dir << 30 | size << 16 | 'E' << 8 | nr`, with `_IOC_READ` = 2 and
    /// `'E'` = 0x45 — rather than from the function under test.
    #[test]
    fn requests_encode_like_the_kernel_macros() {
        assert_eq!(
            evioc_read(EVIOCGID_NR, size_of::<InputId>()),
            0x8008_4502,
            "EVIOCGID"
        );
        assert_eq!(
            evioc_read(eviocgabs_nr(ABS_X), size_of::<AbsInfo>()),
            0x8018_4540,
            "EVIOCGABS(ABS_X)"
        );
        assert_eq!(
            evioc_read(eviocgabs_nr(ABS_HAT0Y), size_of::<AbsInfo>()),
            0x8018_4551,
            "EVIOCGABS(ABS_HAT0Y)"
        );
        assert_eq!(
            evioc_read(eviocgbit_nr(EV_KEY), size_of::<KeyBits>()),
            0x8060_4521,
            "EVIOCGBIT(EV_KEY, 96)"
        );
        assert_eq!(
            evioc_read(eviocgbit_nr(EV_ABS), size_of::<AbsBits>()),
            0x8008_4523,
            "EVIOCGBIT(EV_ABS, 8)"
        );
        assert_eq!(
            evioc_read(EVIOCGKEY_NR, size_of::<KeyBits>()),
            0x8060_4518,
            "EVIOCGKEY(96)"
        );
    }

    /// Bytes laid out the way the kernel writes them decode field by field,
    /// and a torn buffer is refused rather than half-read.
    #[test]
    fn read_bytes_decode_into_events() {
        let word = size_of::<usize>();
        let mut bytes = vec![0; 2 * EVENT_SIZE];
        for (index, (kind, code, value)) in [(EV_ABS, ABS_Y, -32768), (EV_SYN, SYN_REPORT, 0)]
            .into_iter()
            .enumerate()
        {
            let event = &mut bytes[index * EVENT_SIZE..][..EVENT_SIZE];
            if index == 0 {
                event[..word].fill(0xAB);
            }
            event[2 * word..][..2].copy_from_slice(&kind.to_ne_bytes());
            event[2 * word + 2..][..2].copy_from_slice(&code.to_ne_bytes());
            event[2 * word + 4..][..4].copy_from_slice(&i32::to_ne_bytes(value));
        }
        let events: Vec<_> = decode_events(&bytes).expect("whole events").collect();
        assert_eq!(events.len(), 2);
        assert_eq!(
            (events[0].kind, events[0].code, events[0].value),
            (EV_ABS, ABS_Y, -32768)
        );
        assert_eq!(
            events[0].sec,
            KernelUlong::from_ne_bytes([0xAB; size_of::<KernelUlong>()])
        );
        assert_eq!(events[1], InputEvent::new(EV_SYN, SYN_REPORT, 0));

        let torn = decode_events(&bytes[..EVENT_SIZE + 1]).map(|_| ());
        assert_eq!(
            torn.map_err(|error| error.kind()),
            Err(io::ErrorKind::InvalidData)
        );
    }

    /// A bitmap reads back what was set, word boundaries included, and
    /// ignores codes past its end both ways.
    #[test]
    fn bitmaps_read_by_word() {
        let mut keys = KeyBits::EMPTY;
        for code in [0, 63, 64, BTN_SOUTH, 0x2ff] {
            keys.set(code, true);
        }
        for code in [0, 63, 64, BTN_SOUTH, 0x2ff] {
            assert!(keys.has(code), "{code:#x}");
        }
        assert!(!keys.has(1) && !keys.has(BTN_EAST));
        keys.set(BTN_SOUTH, false);
        assert!(!keys.has(BTN_SOUTH));
        keys.set(0x300, true);
        assert!(!keys.has(0x300), "past KEY_MAX is not a key");
    }
}
