//! The callbacks Windows calls when something changes, and the registrations
//! that keep them called.
//!
//! **A callback only signals.** Windows calls these on threads of its own, and
//! its documentation forbids calling back into the audio API from them, so
//! each one sends a [`Message`] down the mixer thread's channel and returns;
//! the thread re-reads. A send to a mixer that has stopped fails, and there is
//! nothing left to tell then.
//!
//! **A registration ends with its guard.** Each `*Watch` holds the object it
//! registered on and the callback it registered, and unregisters in `drop`,
//! on the mixer's thread, so an endpoint or session leaving the mixer's tables
//! stops its callbacks with it.

use std::sync::mpsc::Sender;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, AudioSessionDisconnectReason, AudioSessionState, DEVICE_STATE,
    DisconnectReasonServerShutdown, EDataFlow, ERole, IAudioSessionControl, IAudioSessionEvents,
    IAudioSessionEvents_Impl, IAudioSessionManager2, IAudioSessionNotification,
    IAudioSessionNotification_Impl, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, eConsole,
};
use windows::core::{BOOL, GUID, PCWSTR, Ref, Result};
use windows_core::implement;

use super::Message;

/// Sends `message` to the mixer's thread. A failed send means the thread has
/// stopped, after which nobody is listening for changes.
fn signal(changes: &Sender<Message>, message: Message) {
    let _stopped = changes.send(message);
}

/// Endpoints arriving, leaving, changing state or name, and the default
/// changing.
#[implement(IMMNotificationClient)]
struct DeviceChanges {
    changes: Sender<Message>,
}

impl IMMNotificationClient_Impl for DeviceChanges_Impl {
    fn OnDeviceStateChanged(&self, _device: &PCWSTR, _state: DEVICE_STATE) -> Result<()> {
        signal(&self.changes, Message::Changed);
        Ok(())
    }

    fn OnDeviceAdded(&self, _device: &PCWSTR) -> Result<()> {
        signal(&self.changes, Message::Changed);
        Ok(())
    }

    fn OnDeviceRemoved(&self, _device: &PCWSTR) -> Result<()> {
        signal(&self.changes, Message::Changed);
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        _flow: EDataFlow,
        role: ERole,
        _device: &PCWSTR,
    ) -> Result<()> {
        // The mixer reads the console role's default (see `audio`); a change
        // to another role alone changes nothing it shows.
        if role == eConsole {
            signal(&self.changes, Message::Changed);
        }
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _device: &PCWSTR, key: &PROPERTYKEY) -> Result<()> {
        // Drivers change other properties often; the friendly name is the one
        // the mixer shows that can change while an endpoint stays.
        if *key == PKEY_Device_FriendlyName {
            signal(&self.changes, Message::Changed);
        }
        Ok(())
    }
}

/// An endpoint's volume or mute changing.
#[implement(IAudioEndpointVolumeCallback)]
struct VolumeChanges {
    changes: Sender<Message>,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeChanges_Impl {
    fn OnNotify(&self, _data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        signal(&self.changes, Message::Changed);
        Ok(())
    }
}

/// A session starting on an endpoint.
#[implement(IAudioSessionNotification)]
struct SessionCreated {
    changes: Sender<Message>,
}

impl IAudioSessionNotification_Impl for SessionCreated_Impl {
    fn OnSessionCreated(&self, _session: Ref<'_, IAudioSessionControl>) -> Result<()> {
        signal(&self.changes, Message::Changed);
        Ok(())
    }
}

/// One session's volume, name and state changing, and its end.
#[implement(IAudioSessionEvents)]
struct SessionEvents {
    changes: Sender<Message>,
}

impl IAudioSessionEvents_Impl for SessionEvents_Impl {
    fn OnDisplayNameChanged(&self, _name: &PCWSTR, _context: *const GUID) -> Result<()> {
        signal(&self.changes, Message::Changed);
        Ok(())
    }

    fn OnIconPathChanged(&self, _path: &PCWSTR, _context: *const GUID) -> Result<()> {
        Ok(())
    }

    fn OnSimpleVolumeChanged(
        &self,
        _volume: f32,
        _mute: BOOL,
        _context: *const GUID,
    ) -> Result<()> {
        signal(&self.changes, Message::Changed);
        Ok(())
    }

    fn OnChannelVolumeChanged(
        &self,
        _count: u32,
        _volumes: *const f32,
        _changed: u32,
        _context: *const GUID,
    ) -> Result<()> {
        // The mixer shows a session's master volume, which a channel's does
        // not move.
        Ok(())
    }

    fn OnGroupingParamChanged(&self, _grouping: *const GUID, _context: *const GUID) -> Result<()> {
        Ok(())
    }

    fn OnStateChanged(&self, _state: AudioSessionState) -> Result<()> {
        // An expired session leaves the mixer's state at the next read.
        signal(&self.changes, Message::Changed);
        Ok(())
    }

    fn OnSessionDisconnected(&self, reason: AudioSessionDisconnectReason) -> Result<()> {
        let message = if reason == DisconnectReasonServerShutdown {
            Message::ServiceGone
        } else {
            Message::Changed
        };
        signal(&self.changes, message);
        Ok(())
    }
}

/// The enumerator's endpoint notifications, registered.
pub(super) struct DeviceWatch {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

impl DeviceWatch {
    pub(super) fn register(
        enumerator: &IMMDeviceEnumerator,
        changes: &Sender<Message>,
    ) -> Result<Self> {
        let client: IMMNotificationClient = DeviceChanges {
            changes: changes.clone(),
        }
        .into();
        // SAFETY: `client` is a live COM object; the enumerator takes its own
        // reference until it is unregistered in `drop`.
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }?;
        Ok(Self {
            enumerator: enumerator.clone(),
            client,
        })
    }
}

impl Drop for DeviceWatch {
    fn drop(&mut self) {
        // SAFETY: registered on this enumerator by `register`, unregistered
        // once, from the mixer's thread and not from inside a callback. A
        // failure leaves a client that only sends to a stopped thread.
        let _unregistered = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
        };
    }
}

/// An endpoint volume's change notifications, registered.
pub(super) struct VolumeWatch {
    volume: IAudioEndpointVolume,
    callback: IAudioEndpointVolumeCallback,
}

impl VolumeWatch {
    pub(super) fn register(
        volume: &IAudioEndpointVolume,
        changes: &Sender<Message>,
    ) -> Result<Self> {
        let callback: IAudioEndpointVolumeCallback = VolumeChanges {
            changes: changes.clone(),
        }
        .into();
        // SAFETY: `callback` is a live COM object; the endpoint takes its own
        // reference until it is unregistered in `drop`.
        unsafe { volume.RegisterControlChangeNotify(&callback) }?;
        Ok(Self {
            volume: volume.clone(),
            callback,
        })
    }
}

impl Drop for VolumeWatch {
    fn drop(&mut self) {
        // SAFETY: registered on this endpoint by `register`, unregistered once
        // from the mixer's thread. A failure leaves a callback that only sends
        // to a stopped thread.
        let _unregistered = unsafe { self.volume.UnregisterControlChangeNotify(&self.callback) };
    }
}

/// An endpoint's new-session notifications, registered.
pub(super) struct SessionWatch {
    manager: IAudioSessionManager2,
    notification: IAudioSessionNotification,
}

impl SessionWatch {
    /// Registers for `manager`'s new sessions. Windows reports them only once
    /// the manager's sessions have been enumerated, which the caller has done
    /// (`Endpoint::open`).
    pub(super) fn register(
        manager: &IAudioSessionManager2,
        changes: &Sender<Message>,
    ) -> Result<Self> {
        let notification: IAudioSessionNotification = SessionCreated {
            changes: changes.clone(),
        }
        .into();
        // SAFETY: `notification` is a live COM object; the manager takes its
        // own reference until it is unregistered in `drop`. The call is made
        // from the multithreaded apartment, as Windows requires of it.
        unsafe { manager.RegisterSessionNotification(&notification) }?;
        Ok(Self {
            manager: manager.clone(),
            notification,
        })
    }
}

impl Drop for SessionWatch {
    fn drop(&mut self) {
        // SAFETY: registered on this manager by `register`, unregistered once
        // from the mixer's thread. A failure leaves a notification that only
        // sends to a stopped thread.
        let _unregistered = unsafe {
            self.manager
                .UnregisterSessionNotification(&self.notification)
        };
    }
}

/// One session's events, registered.
pub(super) struct EventsWatch {
    control: IAudioSessionControl,
    events: IAudioSessionEvents,
}

impl EventsWatch {
    pub(super) fn register(
        control: &IAudioSessionControl,
        changes: &Sender<Message>,
    ) -> Result<Self> {
        let events: IAudioSessionEvents = SessionEvents {
            changes: changes.clone(),
        }
        .into();
        // SAFETY: `events` is a live COM object; the session takes its own
        // reference until it is unregistered in `drop`.
        unsafe { control.RegisterAudioSessionNotification(&events) }?;
        Ok(Self {
            control: control.clone(),
            events,
        })
    }
}

impl Drop for EventsWatch {
    fn drop(&mut self) {
        // SAFETY: registered on this session by `register`, unregistered once
        // from the mixer's thread, never from inside one of its callbacks,
        // where Windows' documentation says the call can deadlock. A failure
        // leaves events that only send to a stopped thread.
        let _unregistered = unsafe {
            self.control
                .UnregisterAudioSessionNotification(&self.events)
        };
    }
}
