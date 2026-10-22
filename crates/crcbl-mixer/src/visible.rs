//! What an app's [`Interest`] lets it see of the state, which is what every
//! backend compares to decide whether a change is worth a wake.
//!
//! **A wake is owed only when this view changes.** A backend keeps the view it
//! last woke the app for, builds a new one once a burst of changes has
//! settled, and calls the waker only when the two differ: a stream starting
//! does not wake an app that asked for devices only, and a change that lands
//! back where it started does not wake anyone.

use crate::Interest;
use crate::model::{Card, Device, MixerState, Stream};

/// The part of the state an app with `interest` watches: the streams only
/// when it asked for them.
#[derive(Debug, PartialEq)]
pub(crate) struct Visible {
    devices: Vec<Device>,
    cards: Vec<Card>,
    streams: Option<Vec<Stream>>,
}

impl Visible {
    pub(crate) fn of(state: MixerState, interest: Interest) -> Self {
        Self {
            devices: state.devices,
            cards: state.cards,
            streams: (interest == Interest::DevicesAndStreams).then_some(state.streams),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeviceId, Direction, StreamId};

    fn device(volume: u32) -> Device {
        Device {
            id: DeviceId(1),
            direction: Direction::Output,
            name: "speakers".into(),
            description: "Speakers".into(),
            volume,
            muted: false,
            default: true,
            active_port: None,
            ports: Vec::new(),
            form_factor: Some("speaker".into()),
            bluetooth: false,
            card: None,
        }
    }

    fn stream(volume: u32) -> Stream {
        Stream {
            id: StreamId(7),
            direction: Direction::Output,
            application: "player".into(),
            title: None,
            volume,
            muted: false,
            device: Some(DeviceId(1)),
        }
    }

    fn state(device_volume: u32, streams: Vec<Stream>) -> MixerState {
        MixerState {
            devices: vec![device(device_volume)],
            streams,
            cards: Vec::new(),
        }
    }

    /// **Stream churn is no change to a devices-only app, and is one to a
    /// streams app**: a stream appearing, changing its volume and going, each
    /// compared against the state before it. A device's volume is a change to
    /// both.
    #[test]
    fn a_devices_app_sees_no_stream_churn_and_a_streams_app_does() {
        let quiet = state(50, Vec::new());
        let playing = state(50, vec![stream(100)]);
        let louder = state(50, vec![stream(80)]);
        let turned_down = state(40, Vec::new());
        for (before, after) in [(&quiet, &playing), (&playing, &louder), (&louder, &quiet)] {
            assert_eq!(
                Visible::of(before.clone(), Interest::Devices),
                Visible::of(after.clone(), Interest::Devices),
                "a devices-only app does not see streams"
            );
            assert_ne!(
                Visible::of(before.clone(), Interest::DevicesAndStreams),
                Visible::of(after.clone(), Interest::DevicesAndStreams),
                "a streams app does"
            );
        }
        for interest in [Interest::Devices, Interest::DevicesAndStreams] {
            assert_ne!(
                Visible::of(quiet.clone(), interest),
                Visible::of(turned_down.clone(), interest),
                "a device's volume is a change to every app"
            );
        }
    }
}
