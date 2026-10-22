//! A session the mixer follows: an application's stream on one endpoint, in
//! WASAPI's words, with its volume control and its events.

use std::path::Path;
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{CloseHandle, HANDLE, S_OK};
use windows::Win32::Media::Audio::{
    IAudioSessionControl, IAudioSessionControl2, ISimpleAudioVolume,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::{Interface as _, PWSTR, Result};

use super::Message;
use super::com;
use super::notify::EventsWatch;
use crate::level;
use crate::model::{DeviceId, Direction, Stream, StreamId};

/// The name [`application`] gives Windows' own sounds' session.
const SYSTEM_SOUNDS: &str = "System sounds";

/// The name [`application`] gives a session it can learn nothing about.
const UNKNOWN: &str = "Unknown";

/// The longest path Windows allows, in UTF-16 units with the terminator.
const LONGEST_PATH: usize = 32_768;

/// A session's application name: [`SYSTEM_SOUNDS`] for Windows' own, else
/// the name the session gives itself, else its program's file name without
/// its extension, else [`UNKNOWN`]. A session's own name can be an indirect
/// resource string (`@%SystemRoot%\System32\…`), which is a reference to a
/// name rather than one to show, so it falls through to the program's.
pub(super) fn application(
    system_sounds: bool,
    display: Option<&str>,
    program: Option<&str>,
) -> String {
    if system_sounds {
        return SYSTEM_SOUNDS.to_owned();
    }
    if let Some(display) = display.filter(|name| !name.is_empty() && !name.starts_with('@')) {
        return display.to_owned();
    }
    program.unwrap_or(UNKNOWN).to_owned()
}

/// A process handle, closed when dropped.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: a handle `OpenProcess` returned, closed once. A failed close
        // leaks one handle and changes nothing else.
        let _closed = unsafe { CloseHandle(self.0) };
    }
}

/// The file name, without its extension, of the program process `pid` runs,
/// when it can be read: a protected process's or another user's cannot.
fn program(pid: u32) -> Option<String> {
    // SAFETY: a plain call; the handle is checked and owned by `Process`.
    let process =
        Process(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let mut path = vec![0_u16; LONGEST_PATH];
    let mut length = u32::try_from(path.len()).ok()?;
    // SAFETY: the buffer and the length passed agree, and the call writes no
    // more than that length; it then sets `length` to what it wrote.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    }
    .ok()?;
    let path = String::from_utf16_lossy(path.get(..usize::try_from(length).ok()?)?);
    Path::new(&path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// A session's instance identifier, which keys the mixer's table of
/// sessions: unique to the session, across every endpoint.
pub(super) fn key(control: &IAudioSessionControl) -> Option<String> {
    let control: IAudioSessionControl2 = control.cast().ok()?;
    // SAFETY: a plain read; `take` frees the string COM allocated.
    com::take(unsafe { control.GetSessionInstanceIdentifier() }.ok()?)
}

/// A session the mixer follows, and the registration that tells it of the
/// session's changes. Dropping it unregisters that.
pub(super) struct Session {
    direction: Direction,
    /// The endpoint it is on, which it never leaves: Windows has no public
    /// way to move a session.
    pub(super) device: DeviceId,
    control: IAudioSessionControl2,
    volume: ISimpleAudioVolume,
    system_sounds: bool,
    /// Its program's name, read once: a session belongs to one process for
    /// its life.
    program: Option<String>,
    _events: EventsWatch,
}

impl Session {
    pub(super) fn open(
        control: &IAudioSessionControl,
        device: DeviceId,
        direction: Direction,
        changes: &Sender<Message>,
    ) -> Result<Self> {
        let control2: IAudioSessionControl2 = control.cast()?;
        let volume: ISimpleAudioVolume = control.cast()?;
        // SAFETY: plain reads.
        let system_sounds = unsafe { control2.IsSystemSoundsSession() } == S_OK;
        let program = if system_sounds {
            None
        } else {
            // SAFETY: a plain read.
            unsafe { control2.GetProcessId() }.ok().and_then(program)
        };
        Ok(Self {
            direction,
            device,
            control: control2,
            volume,
            system_sounds,
            program,
            _events: EventsWatch::register(control, changes)?,
        })
    }

    pub(super) fn volume(&self) -> &ISimpleAudioVolume {
        &self.volume
    }

    /// The session as the mixer reports it. Windows says nothing of what a
    /// session is playing, so it has no title.
    pub(super) fn stream(&self, id: StreamId) -> Result<Stream> {
        // SAFETY: plain reads; `take` frees the name COM allocated.
        let (display, scalar, muted) = unsafe {
            (
                self.control.GetDisplayName().ok().and_then(com::take),
                self.volume.GetMasterVolume()?,
                self.volume.GetMute()?.as_bool(),
            )
        };
        Ok(Stream {
            id,
            direction: self.direction,
            application: application(
                self.system_sounds,
                display.as_deref(),
                self.program.as_deref(),
            ),
            title: None,
            volume: level::percent_from_scalar(scalar),
            muted,
            device: Some(self.device),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_is_named_by_itself_else_by_its_program() {
        assert_eq!(
            application(true, Some("@%SystemRoot%\\x.dll,-1"), None),
            SYSTEM_SOUNDS
        );
        assert_eq!(
            application(false, Some("Spotify"), Some("Spotify")),
            "Spotify"
        );
        assert_eq!(application(false, Some(""), Some("firefox")), "firefox");
        assert_eq!(
            application(false, Some("@%SystemRoot%\\x.dll,-1"), Some("app")),
            "app",
            "a resource string is not a name"
        );
        assert_eq!(application(false, None, None), UNKNOWN);
    }

    /// This test's own process is one whose program can be read.
    #[test]
    fn a_process_s_program_is_its_file_name_without_the_extension() {
        let expected = std::env::current_exe()
            .unwrap()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(program(std::process::id()), Some(expected));
    }
}
