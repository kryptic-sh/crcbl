//! What the system mixer has: devices, streams and cards, as a snapshot.
//!
//! Plain data, the same on every platform. A backend builds a [`MixerState`]
//! from what the sound server or the OS reports and hands out clones of it; a
//! caller reads one, decides, and sends an [`Action`](crate::Action) back.

/// Which way audio flows through a device or stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Playback: a device that plays sound, a stream that sends it.
    Output,
    /// Capture: a device that records, a stream that receives.
    Input,
}

/// A device's identity within one [`Mixer`](crate::Mixer) session.
///
/// The sound server's own id for the object on Linux, and on Windows one the
/// mixer hands out the first time it sees the endpoint (Windows names
/// endpoints by string). Either way it is stable while the device stays
/// connected and means nothing to another session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u32);

/// A stream's identity within one session; see [`DeviceId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId(pub u32);

/// A sound card's identity within one session; see [`DeviceId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CardId(pub u32);

/// One of a set of alternatives: a card's profile or a device's port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The server's name for it, which is what selecting it sends back.
    pub name: String,
    /// The name a person reads.
    pub description: String,
    /// Whether it can be used now: a headphone port with nothing plugged in
    /// is not.
    pub available: bool,
}

/// An output or input device: a sink or a source, in PulseAudio's words.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: DeviceId,
    pub direction: Direction,
    /// The server's name, such as `alsa_output.pci-0000_00_1f.3.analog-stereo`.
    pub name: String,
    /// The name a person reads, such as "Built-in Audio Analog Stereo".
    pub description: String,
    /// Percent of the device's nominal level: 100 is unity, and above it is a
    /// software boost where the platform allows one (see
    /// [`MAX_PERCENT`](crate::level::MAX_PERCENT)); Windows allows none.
    pub volume: u32,
    pub muted: bool,
    /// Whether this is the default device for its direction.
    pub default: bool,
    /// The port the device is using now, by its [`Choice::name`], if it has
    /// ports.
    pub active_port: Option<String>,
    /// Every port the device can switch to. Empty where a platform reports
    /// none, which Windows and macOS do.
    pub ports: Vec<Choice>,
    /// The form factor the server reports, such as `headset` or `speaker`.
    /// On Windows, the endpoint's form factor in the same words (see the
    /// Windows backend's `form_factor`).
    pub form_factor: Option<String>,
    /// Whether the device is a Bluetooth one.
    pub bluetooth: bool,
    /// The card the device belongs to, if it belongs to one.
    pub card: Option<CardId>,
}

/// An application's playing or recording stream.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub id: StreamId,
    /// [`Direction::Output`] for a stream that plays, [`Direction::Input`] for
    /// one that records.
    pub direction: Direction,
    /// The application that owns it, such as "Firefox".
    pub application: String,
    /// What it is playing or recording, where the application says.
    pub title: Option<String>,
    /// Percent; see [`Device::volume`].
    pub volume: u32,
    pub muted: bool,
    /// The device it is connected to, if it is connected.
    pub device: Option<DeviceId>,
}

/// A sound card: the hardware behind one or more devices, and its profiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub id: CardId,
    pub name: String,
    pub description: String,
    /// The profile in use, by its [`Choice::name`].
    pub active_profile: Option<String>,
    /// Every profile the card offers.
    pub profiles: Vec<Choice>,
}

/// Everything the mixer reports, at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MixerState {
    /// Every output and input device, monitors excluded.
    pub devices: Vec<Device>,
    /// Every playing and recording stream.
    pub streams: Vec<Stream>,
    /// Every sound card. Empty where a platform reports none.
    pub cards: Vec<Card>,
}

impl MixerState {
    /// The default device for `direction`, if there is one.
    #[must_use]
    pub fn default_device(&self, direction: Direction) -> Option<&Device> {
        self.devices
            .iter()
            .find(|device| device.direction == direction && device.default)
    }

    /// The device `id`, if it is here.
    #[must_use]
    pub fn device(&self, id: DeviceId) -> Option<&Device> {
        self.devices.iter().find(|device| device.id == id)
    }

    /// The stream `id`, if it is here.
    #[must_use]
    pub fn stream(&self, id: StreamId) -> Option<&Stream> {
        self.streams.iter().find(|stream| stream.id == id)
    }
}
