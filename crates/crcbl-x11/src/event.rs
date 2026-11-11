//! What [`Connection::poll_event`](crate::Connection::poll_event) returns:
//! RandR's events decoded, everything else named and passed through.
//!
//! RandR's event numbers are not fixed: the server assigns the extension a
//! block starting at its `first_event`, and `ScreenChangeNotify` is
//! `first_event + 0`, `Notify` `first_event + 1`. Decoding against the wrong
//! base would read an unrelated core event as a RandR one, so the base is the
//! one the connection's own `QueryExtension` returned, and without RandR
//! nothing is decoded as RandR.

use core::ptr;

use crate::connection::{Atom, Plain, Timestamp, Window, XError};
use crate::ffi::{
    GenericError, GenericEvent, RandrCrtcChange, RandrNotifyEvent, RandrOutputChange,
    RandrOutputProperty, RandrScreenChangeNotifyEvent, randr_notify,
};
use crate::randr::{Crtc, Mode, Output, OutputConnection, Rotation, SubPixel};

/// One event off the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// RandR's `ScreenChangeNotify`: the screen's size or rotation changed.
    ScreenChange(ScreenChangeNotify),
    /// RandR's `Notify` with `CrtcChange`.
    CrtcChange(CrtcChangeNotify),
    /// RandR's `Notify` with `OutputChange`.
    OutputChange(OutputChangeNotify),
    /// RandR's `Notify` with `OutputProperty`.
    OutputProperty(OutputPropertyNotify),
    /// An X error delivered as an event: one from a request sent unchecked.
    /// The connection is still usable.
    Error(XError),
    /// RandR's `Notify` with a sub-code this crate does not decode —
    /// providers, resources, leases.
    OtherRandrNotify {
        /// The `Notify` sub-code.
        sub_code: u8,
    },
    /// Any other event.
    Other {
        /// Its event number, with the `SendEvent` bit cleared.
        response_type: u8,
    },
}

/// `ScreenChangeNotify`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScreenChangeNotify {
    /// When the change was made.
    pub timestamp: Timestamp,
    /// When the server last changed its configuration.
    pub config_timestamp: Timestamp,
    /// The root window of the screen that changed.
    pub root: Window,
    /// The window the event was selected on.
    pub request_window: Window,
    /// The screen's rotation.
    pub rotation: Rotation,
    /// Index into the RandR 1.0 size list.
    pub size_id: u16,
    /// Subpixel order.
    pub subpixel: SubPixel,
    /// New width in pixels.
    pub width: u16,
    /// New height in pixels.
    pub height: u16,
    /// New width in millimetres.
    pub mm_width: u16,
    /// New height in millimetres.
    pub mm_height: u16,
}

/// `Notify` with `CrtcChange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CrtcChangeNotify {
    /// When the change was made.
    pub timestamp: Timestamp,
    /// The window the event was selected on.
    pub window: Window,
    /// The CRTC that changed.
    pub crtc: Crtc,
    /// Its mode now; `0` when it was disabled.
    pub mode: Mode,
    /// Its rotation.
    pub rotation: Rotation,
    /// Left edge in the screen.
    pub x: i16,
    /// Top edge.
    pub y: i16,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
}

/// `Notify` with `OutputChange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutputChangeNotify {
    /// When the change was made.
    pub timestamp: Timestamp,
    /// When the server last changed its configuration.
    pub config_timestamp: Timestamp,
    /// The window the event was selected on.
    pub window: Window,
    /// The output that changed.
    pub output: Output,
    /// The CRTC it is on, or `0`.
    pub crtc: Crtc,
    /// The CRTC's mode, or `0`.
    pub mode: Mode,
    /// The CRTC's rotation.
    pub rotation: Rotation,
    /// Whether a monitor is attached.
    pub connection: OutputConnection,
    /// Subpixel order.
    pub subpixel: SubPixel,
}

/// Whether an output property was set or deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyState {
    /// It has a new value.
    NewValue,
    /// It was deleted.
    Deleted,
    /// A value this crate does not know.
    Other(u8),
}

/// `Notify` with `OutputProperty`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutputPropertyNotify {
    /// The window the event was selected on.
    pub window: Window,
    /// The output whose property changed.
    pub output: Output,
    /// Which property.
    pub atom: Atom,
    /// When it changed.
    pub timestamp: Timestamp,
    /// Set or deleted.
    pub state: PropertyState,
}

/// The bit `SendEvent` sets in a synthetic event's number.
const SEND_EVENT: u8 = 0x80;

/// Decodes `event`, RandR's events against `randr_first_event` when the
/// connection has RandR.
pub(crate) fn decode(event: &GenericEvent, randr_first_event: Option<u8>) -> Event {
    let response_type = event.response_type & !SEND_EVENT;
    if response_type == 0 {
        return Event::Error(XError::from_raw(&reinterpret::<GenericError>(event)));
    }
    let Some(offset) = randr_first_event.and_then(|first| response_type.checked_sub(first)) else {
        return Event::Other { response_type };
    };
    match offset {
        0 => {
            let raw: RandrScreenChangeNotifyEvent = reinterpret(event);
            Event::ScreenChange(ScreenChangeNotify {
                timestamp: raw.timestamp,
                config_timestamp: raw.config_timestamp,
                root: raw.root,
                request_window: raw.request_window,
                rotation: Rotation(u16::from(raw.rotation)),
                size_id: raw.size_id,
                subpixel: SubPixel::from_raw(raw.subpixel_order),
                width: raw.width,
                height: raw.height,
                mm_width: raw.mwidth,
                mm_height: raw.mheight,
            })
        }
        1 => decode_notify(event),
        _ => Event::Other { response_type },
    }
}

/// Decodes RandR's `Notify` by its sub-code.
fn decode_notify(event: &GenericEvent) -> Event {
    // Every arm shares the 4-byte header, so the sub-code is read through any.
    let sub_code = reinterpret::<RandrNotifyEvent<RandrCrtcChange>>(event).sub_code;
    match sub_code {
        randr_notify::CRTC_CHANGE => {
            let raw = reinterpret::<RandrNotifyEvent<RandrCrtcChange>>(event).body;
            Event::CrtcChange(CrtcChangeNotify {
                timestamp: raw.timestamp,
                window: raw.window,
                crtc: raw.crtc,
                mode: raw.mode,
                rotation: Rotation(raw.rotation),
                x: raw.x,
                y: raw.y,
                width: raw.width,
                height: raw.height,
            })
        }
        randr_notify::OUTPUT_CHANGE => {
            let raw = reinterpret::<RandrNotifyEvent<RandrOutputChange>>(event).body;
            Event::OutputChange(OutputChangeNotify {
                timestamp: raw.timestamp,
                config_timestamp: raw.config_timestamp,
                window: raw.window,
                output: raw.output,
                crtc: raw.crtc,
                mode: raw.mode,
                rotation: Rotation(raw.rotation),
                connection: OutputConnection::from_raw(raw.connection),
                subpixel: SubPixel::from_raw(u16::from(raw.subpixel_order)),
            })
        }
        randr_notify::OUTPUT_PROPERTY => {
            let raw = reinterpret::<RandrNotifyEvent<RandrOutputProperty>>(event).body;
            Event::OutputProperty(OutputPropertyNotify {
                window: raw.window,
                output: raw.output,
                atom: raw.atom,
                timestamp: raw.timestamp,
                state: match raw.status {
                    0 => PropertyState::NewValue,
                    1 => PropertyState::Deleted,
                    other => PropertyState::Other(other),
                },
            })
        }
        sub_code => Event::OtherRandrNotify { sub_code },
    }
}

// SAFETY: each is a `#[repr(C)]` struct of integers (or of such structs),
// transcribed from libxcb's headers with their padding as named fields; their
// sizes are pinned in `ffi`'s layout test.
unsafe impl Plain for GenericEvent {}
// SAFETY: as above.
unsafe impl Plain for GenericError {}
// SAFETY: as above.
unsafe impl Plain for RandrScreenChangeNotifyEvent {}
// SAFETY: as above.
unsafe impl Plain for RandrNotifyEvent<RandrCrtcChange> {}
// SAFETY: as above.
unsafe impl Plain for RandrNotifyEvent<RandrOutputChange> {}
// SAFETY: as above.
unsafe impl Plain for RandrNotifyEvent<RandrOutputProperty> {}

/// Reads `event`'s bytes as `T`: how a C client casts
/// `xcb_generic_event_t *` to the specific event, without the cast.
fn reinterpret<T: Plain>(event: &GenericEvent) -> T {
    const {
        assert!(size_of::<T>() <= size_of::<GenericEvent>());
        assert!(align_of::<T>() <= align_of::<GenericEvent>());
    };
    // SAFETY: `T` fits inside `GenericEvent` and needs no more alignment (the
    // assertions above, at compile time), and every bit pattern is a valid `T`
    // (`Plain`'s contract), so reading `T` from the start of a live
    // `GenericEvent` reads initialised bytes into a valid value.
    unsafe { ptr::from_ref(event).cast::<T>().read() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RandR's base in these tests: an arbitrary number, as the server's is.
    const FIRST: u8 = 89;

    /// `value`'s bytes as the generic event libxcb would hand over.
    fn generic<T: Plain>(value: T) -> GenericEvent {
        const {
            assert!(size_of::<T>() <= size_of::<GenericEvent>());
            assert!(align_of::<T>() <= align_of::<GenericEvent>());
        };
        let mut event = GenericEvent {
            response_type: 0,
            pad0: 0,
            sequence: 0,
            pad: [0; 7],
            full_sequence: 0,
        };
        // SAFETY: as in `reinterpret`, the other way: `T` fits inside the
        // event at its start, and any bytes are a valid `GenericEvent`.
        unsafe { ptr::from_mut(&mut event).cast::<T>().write(value) };
        event
    }

    #[test]
    fn a_screen_change_is_decoded_field_by_field() {
        let event = generic(RandrScreenChangeNotifyEvent {
            response_type: FIRST,
            rotation: 2,
            timestamp: 11,
            config_timestamp: 12,
            root: 0x100,
            request_window: 0x101,
            size_id: 3,
            subpixel_order: 1,
            width: 1024,
            height: 768,
            mwidth: 270,
            mheight: 203,
            ..RandrScreenChangeNotifyEvent::default()
        });
        assert_eq!(
            decode(&event, Some(FIRST)),
            Event::ScreenChange(ScreenChangeNotify {
                timestamp: 11,
                config_timestamp: 12,
                root: 0x100,
                request_window: 0x101,
                rotation: Rotation::ROTATE_90,
                size_id: 3,
                subpixel: SubPixel::HorizontalRgb,
                width: 1024,
                height: 768,
                mm_width: 270,
                mm_height: 203,
            })
        );
    }

    #[test]
    fn a_crtc_change_is_decoded_field_by_field() {
        let event = generic(RandrNotifyEvent {
            response_type: FIRST + 1,
            sub_code: randr_notify::CRTC_CHANGE,
            sequence: 0,
            body: RandrCrtcChange {
                timestamp: 21,
                window: 0x100,
                crtc: 0x3b,
                mode: 0x3a9,
                rotation: 1,
                x: -5,
                y: 7,
                width: 1280,
                height: 720,
                ..RandrCrtcChange::default()
            },
        });
        assert_eq!(
            decode(&event, Some(FIRST)),
            Event::CrtcChange(CrtcChangeNotify {
                timestamp: 21,
                window: 0x100,
                crtc: 0x3b,
                mode: 0x3a9,
                rotation: Rotation::ROTATE_0,
                x: -5,
                y: 7,
                width: 1280,
                height: 720,
            })
        );
    }

    #[test]
    fn an_output_change_is_decoded_with_its_connection() {
        let event = generic(RandrNotifyEvent {
            response_type: FIRST + 1,
            sub_code: randr_notify::OUTPUT_CHANGE,
            sequence: 0,
            body: RandrOutputChange {
                timestamp: 31,
                config_timestamp: 32,
                window: 0x100,
                output: 0x3c,
                crtc: 0x3b,
                mode: 0x3a,
                rotation: 1,
                connection: 1,
                subpixel_order: 5,
            },
        });
        assert_eq!(
            decode(&event, Some(FIRST)),
            Event::OutputChange(OutputChangeNotify {
                timestamp: 31,
                config_timestamp: 32,
                window: 0x100,
                output: 0x3c,
                crtc: 0x3b,
                mode: 0x3a,
                rotation: Rotation::ROTATE_0,
                connection: OutputConnection::Disconnected,
                subpixel: SubPixel::None,
            })
        );
    }

    #[test]
    fn an_output_property_change_is_decoded_with_its_state() {
        let event = generic(RandrNotifyEvent {
            response_type: FIRST + 1,
            sub_code: randr_notify::OUTPUT_PROPERTY,
            sequence: 0,
            body: RandrOutputProperty {
                window: 0x100,
                output: 0x3c,
                atom: 300,
                timestamp: 41,
                status: 1,
                ..RandrOutputProperty::default()
            },
        });
        assert_eq!(
            decode(&event, Some(FIRST)),
            Event::OutputProperty(OutputPropertyNotify {
                window: 0x100,
                output: 0x3c,
                atom: 300,
                timestamp: 41,
                state: PropertyState::Deleted,
            })
        );
    }

    #[test]
    fn a_notify_sub_code_not_decoded_is_named() {
        let event = generic(RandrNotifyEvent {
            response_type: FIRST + 1,
            sub_code: 5,
            sequence: 0,
            body: RandrCrtcChange::default(),
        });
        assert_eq!(
            decode(&event, Some(FIRST)),
            Event::OtherRandrNotify { sub_code: 5 }
        );
    }

    #[test]
    fn an_error_event_is_decoded_as_an_x_error() {
        let event = generic(GenericError {
            response_type: 0,
            error_code: 8,
            sequence: 0,
            resource_id: 0x3b,
            minor_code: 21,
            major_code: 140,
            pad0: 0,
            pad: [0; 5],
            full_sequence: 77,
        });
        assert_eq!(
            decode(&event, Some(FIRST)),
            Event::Error(XError {
                error_code: 8,
                major_opcode: 140,
                minor_opcode: 21,
                resource_id: 0x3b,
                sequence: 77,
            })
        );
    }

    #[test]
    fn events_outside_randrs_block_or_without_randr_pass_through() {
        let event = |response_type| {
            generic(RandrScreenChangeNotifyEvent {
                response_type,
                ..RandrScreenChangeNotifyEvent::default()
            })
        };
        // A core `ConfigureNotify`, below the block.
        assert_eq!(
            decode(&event(22), Some(FIRST)),
            Event::Other { response_type: 22 }
        );
        // Past `Notify`.
        assert_eq!(
            decode(&event(FIRST + 2), Some(FIRST)),
            Event::Other {
                response_type: FIRST + 2
            }
        );
        // RandR's number on a connection that has no RandR.
        assert_eq!(
            decode(&event(FIRST), None),
            Event::Other {
                response_type: FIRST
            }
        );
    }

    #[test]
    fn a_synthetic_event_is_decoded_as_the_real_one() {
        let event = generic(RandrScreenChangeNotifyEvent {
            response_type: FIRST | SEND_EVENT,
            width: 640,
            ..RandrScreenChangeNotifyEvent::default()
        });
        let Event::ScreenChange(change) = decode(&event, Some(FIRST)) else {
            panic!("the SendEvent bit hid the event's number");
        };
        assert_eq!(change.width, 640);
    }
}
