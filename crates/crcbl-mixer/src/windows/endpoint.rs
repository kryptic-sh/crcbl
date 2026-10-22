//! An endpoint the mixer follows: an output or input device, in WASAPI's
//! words, with its volume control, its session manager and their change
//! notifications.

use std::sync::mpsc::Sender;

use windows::Win32::Devices::FunctionDiscovery::{
    PKEY_Device_EnumeratorName, PKEY_Device_FriendlyName,
};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    AudioSessionStateExpired, DigitalAudioDisplayDevice, EndpointFormFactor, Handset, Headphones,
    Headset, IAudioSessionControl, IAudioSessionManager2, IMMDevice, Microphone,
    PKEY_AudioEndpoint_FormFactor, Speakers,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CLSCTX_ALL, STGM_READ};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::Result;

use super::Message;
use super::com;
use super::notify::{SessionWatch, VolumeWatch};
use crate::level;
use crate::model::{Device, DeviceId, Direction};

/// The words [`Device::form_factor`] uses for an endpoint's
/// `PKEY_AudioEndpoint_FormFactor`, the same words PulseAudio and PipeWire
/// use for theirs (`device.form_factor`) where Windows has the same kind:
///
/// | `EndpointFormFactor`        | form factor  |
/// |-----------------------------|--------------|
/// | `Speakers`                  | `speaker`    |
/// | `Headphones`                | `headphone`  |
/// | `Headset`                   | `headset`    |
/// | `Handset`                   | `handset`    |
/// | `Microphone`                | `microphone` |
/// | `DigitalAudioDisplayDevice` | `hdmi`       |
///
/// `hdmi` is Windows' own: Linux reports a display's audio by its port, not
/// its form factor, and HDMI and DisplayPort are both this one value. Line
/// level, S/PDIF, digital passthrough, a remote network device and an unknown
/// form factor report none.
pub(super) fn form_factor(value: EndpointFormFactor) -> Option<&'static str> {
    [
        (Speakers, "speaker"),
        (Headphones, "headphone"),
        (Headset, "headset"),
        (Handset, "handset"),
        (Microphone, "microphone"),
        (DigitalAudioDisplayDevice, "hdmi"),
    ]
    .into_iter()
    .find_map(|(form, word)| (form == value).then_some(word))
}

/// Whether an endpoint's device is on Bluetooth, by the bus enumerator its
/// property store names (`PKEY_Device_EnumeratorName`): Windows' Bluetooth
/// enumerators for classic audio, hands-free and LE audio. The store names the
/// device's bus rather than the endpoint's own software device (`USB` for a
/// USB headset, `HDAUDIO` for a motherboard's codec), as lyr found it.
pub(super) fn bluetooth(enumerator: &str) -> bool {
    ["BTHENUM", "BTHHFENUM", "BTHLEDEVICE"]
        .iter()
        .any(|bus| bus.eq_ignore_ascii_case(enumerator))
}

/// An endpoint's id string, which names it to the enumerator and keys the
/// mixer's table of endpoints.
pub(super) fn key(device: &IMMDevice) -> Option<String> {
    // SAFETY: a plain read; `take` frees the string COM allocated.
    com::take(unsafe { device.GetId() }.ok()?)
}

/// A property from `store`, `None` when the endpoint has no such property.
fn property(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<PROPVARIANT> {
    // SAFETY: a read of a constant key; the value is owned by the caller and
    // cleared when it drops.
    unsafe { store.GetValue(key) }.ok()
}

/// An endpoint the mixer follows, and the registrations that tell it of the
/// endpoint's changes. Dropping it unregisters them.
pub(super) struct Endpoint {
    pub(super) direction: Direction,
    device: IMMDevice,
    volume: IAudioEndpointVolume,
    sessions: IAudioSessionManager2,
    _volume_watch: VolumeWatch,
    _session_watch: SessionWatch,
}

impl Endpoint {
    /// Activates `device`'s volume control and session manager and registers
    /// for both kinds of change.
    pub(super) fn open(
        device: IMMDevice,
        direction: Direction,
        changes: &Sender<Message>,
    ) -> Result<Self> {
        // SAFETY: no activation parameters; each interface is checked by the
        // `windows` crate against the IID it asks for.
        let (volume, sessions) = unsafe {
            (
                device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?,
                device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)?,
            )
        };
        // Windows reports a manager's new sessions only once its sessions have
        // been enumerated.
        // SAFETY: a plain call; the enumerator is dropped unused.
        unsafe { sessions.GetSessionEnumerator() }?;
        let volume_watch = VolumeWatch::register(&volume, changes)?;
        let session_watch = SessionWatch::register(&sessions, changes)?;
        Ok(Self {
            direction,
            device,
            volume,
            sessions,
            _volume_watch: volume_watch,
            _session_watch: session_watch,
        })
    }

    pub(super) fn volume(&self) -> &IAudioEndpointVolume {
        &self.volume
    }

    /// The endpoint as the mixer reports it.
    pub(super) fn device(&self, id: DeviceId, key: &str, default: bool) -> Result<Device> {
        // SAFETY: plain reads.
        let (scalar, muted) = unsafe {
            (
                self.volume.GetMasterVolumeLevelScalar()?,
                self.volume.GetMute()?.as_bool(),
            )
        };
        // SAFETY: read-only access.
        let store = unsafe { self.device.OpenPropertyStore(STGM_READ) }?;
        let text = |key| property(&store, key).map(|value| value.to_string());
        let form = property(&store, &PKEY_AudioEndpoint_FormFactor)
            .and_then(|value| u32::try_from(&value).ok())
            .and_then(|value| i32::try_from(value).ok())
            .and_then(|value| form_factor(EndpointFormFactor(value)));
        Ok(Device {
            id,
            direction: self.direction,
            name: key.to_owned(),
            description: text(&PKEY_Device_FriendlyName)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| key.to_owned()),
            volume: level::percent_from_scalar(scalar),
            muted,
            default,
            active_port: None,
            ports: Vec::new(),
            form_factor: form.map(str::to_owned),
            bluetooth: text(&PKEY_Device_EnumeratorName).is_some_and(|bus| bluetooth(&bus)),
            card: None,
        })
    }

    /// The endpoint's sessions that have not expired: a session whose
    /// program is open but quiet is one, as Windows' own volume mixer lists
    /// it.
    pub(super) fn sessions(&self) -> Result<Vec<IAudioSessionControl>> {
        // SAFETY: plain calls and reads; the index stays below the count the
        // enumerator reports.
        unsafe {
            let listed = self.sessions.GetSessionEnumerator()?;
            let mut sessions = Vec::new();
            for index in 0..listed.GetCount()? {
                let control = listed.GetSession(index)?;
                if control.GetState()? != AudioSessionStateExpired {
                    sessions.push(control);
                }
            }
            Ok(sessions)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::Audio::{LineLevel, SPDIF, UnknownFormFactor};

    #[test]
    fn a_form_factor_is_the_word_linux_uses() {
        assert_eq!(form_factor(Speakers), Some("speaker"));
        assert_eq!(form_factor(Headphones), Some("headphone"));
        assert_eq!(form_factor(Headset), Some("headset"));
        assert_eq!(form_factor(Handset), Some("handset"));
        assert_eq!(form_factor(Microphone), Some("microphone"));
        assert_eq!(form_factor(DigitalAudioDisplayDevice), Some("hdmi"));
        assert_eq!(form_factor(LineLevel), None);
        assert_eq!(form_factor(SPDIF), None);
        assert_eq!(form_factor(UnknownFormFactor), None);
    }

    #[test]
    fn a_bluetooth_bus_marks_the_endpoint_bluetooth() {
        assert!(bluetooth("BTHENUM"));
        assert!(bluetooth("BTHHFENUM"), "hands-free");
        assert!(bluetooth("BTHLEDEVICE"), "LE audio");
        assert!(bluetooth("bthenum"), "names are not case-sensitive");
        assert!(!bluetooth("USB"));
        assert!(!bluetooth("HDAUDIO"));
        assert!(!bluetooth(""));
    }
}
