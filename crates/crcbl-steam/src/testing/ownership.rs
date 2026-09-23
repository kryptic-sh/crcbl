//! The fake half of `ISteamApps` slice 11 binds, and the fake
//! `ISteamRemotePlay`: scripted answers, and every buffer size offered.

use std::ffi::{CStr, c_char, c_void};

use super::{accessor, script};
use crate::ffi::{ISteamApps, ISteamRemotePlay, SteamApiCall, manifest::RemotePlayFns};

/// One scripted beta branch.
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeBeta {
    pub(crate) flags: u32,
    pub(crate) build: u32,
    pub(crate) name: Vec<u8>,
    pub(crate) description: Vec<u8>,
    pub(crate) updated: u32,
}

/// What the fake answers for ownership, DLC and betas.
#[derive(Debug, Default)]
pub(crate) struct FakeApps {
    /// Answered by every ownership `bool` call, by name.
    pub(crate) yes: Vec<&'static str>,
    pub(crate) owned_apps: Vec<u32>,
    pub(crate) purchase_time: u32,
    pub(crate) owner: u64,
    pub(crate) build: i32,
    /// `(app, available, name)` per DLC index.
    pub(crate) dlcs: Vec<(u32, bool, Vec<u8>)>,
    pub(crate) installed_dlc: Vec<u32>,
    /// Every `InstallDLC` / `UninstallDLC`: `(installed, app)`.
    pub(crate) dlc_changes: Vec<(bool, u32)>,
    /// The current branch; `None` makes `GetCurrentBetaName` answer false.
    pub(crate) current_beta: Option<Vec<u8>>,
    pub(crate) betas: Vec<FakeBeta>,
    pub(crate) available_betas: i32,
    pub(crate) private_betas: i32,
    pub(crate) active_beta: Option<String>,
    /// Every `MarkContentCorrupt` argument.
    pub(crate) corrupt: Vec<bool>,
    /// The install directory of every app; empty is not installed.
    pub(crate) install_dir: Vec<u8>,
    /// Every buffer size a string call was offered, by call.
    pub(crate) offered: Vec<(&'static str, usize)>,
    /// What `GetFileDetails` answers, and the name it was asked about.
    pub(crate) file_call: SteamApiCall,
    pub(crate) file_asked: Option<String>,
}

/// What the fake answers for Remote Play.
#[derive(Debug, Default)]
pub(crate) struct FakeRemotePlay {
    /// `(session, together, user, guest, name, form factor, resolution)`.
    pub(crate) sessions: Vec<FakeSession>,
    pub(crate) invites: Vec<u64>,
    pub(crate) panels: u32,
}

/// One scripted session.
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeSession {
    pub(crate) id: u32,
    pub(crate) together: bool,
    pub(crate) user: u64,
    pub(crate) guest: u32,
    /// `None` answers null.
    pub(crate) name: Option<&'static CStr>,
    pub(crate) form_factor: i32,
    pub(crate) resolution: Option<(i32, i32)>,
}

/// Writes as much of `text` as fits before a NUL in `capacity` bytes, as
/// Steam's string copies do, recording the size offered.
fn copy_out(call: &'static str, text: &[u8], out: *mut c_char, capacity: usize) {
    script(|s| s.apps_extra.offered.push((call, capacity)));
    let Some(room) = capacity.checked_sub(1) else {
        return;
    };
    let n = text.len().min(room);
    // SAFETY: every caller passes `capacity` writable bytes, and `n + 1` is at
    // most that.
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), out.cast::<u8>(), n);
        out.add(n).cast::<u8>().write(0);
    }
}

fn says(name: &'static str) -> bool {
    script(|s| s.apps_extra.yes.contains(&name))
}

pub(super) unsafe extern "C" fn fake_is_low_violence(_: *mut ISteamApps) -> bool {
    says("low violence")
}

pub(super) unsafe extern "C" fn fake_is_vac_banned(_: *mut ISteamApps) -> bool {
    says("vac banned")
}

pub(super) unsafe extern "C" fn fake_is_subscribed_from_free_weekend(_: *mut ISteamApps) -> bool {
    says("free weekend")
}

pub(super) unsafe extern "C" fn fake_is_subscribed_from_family_sharing(_: *mut ISteamApps) -> bool {
    says("family shared")
}

pub(super) unsafe extern "C" fn fake_is_subscribed_app(_: *mut ISteamApps, app: u32) -> bool {
    script(|s| s.apps_extra.owned_apps.contains(&app))
}

pub(super) unsafe extern "C" fn fake_is_app_installed(_: *mut ISteamApps, app: u32) -> bool {
    script(|s| s.apps_extra.owned_apps.contains(&app) && !s.apps_extra.install_dir.is_empty())
}

pub(super) unsafe extern "C" fn fake_get_earliest_purchase_unix_time(
    _: *mut ISteamApps,
    _: u32,
) -> u32 {
    script(|s| s.apps_extra.purchase_time)
}

pub(super) unsafe extern "C" fn fake_get_app_owner(_: *mut ISteamApps) -> u64 {
    script(|s| s.apps_extra.owner)
}

pub(super) unsafe extern "C" fn fake_get_app_build_id(_: *mut ISteamApps) -> i32 {
    script(|s| s.apps_extra.build)
}

pub(super) unsafe extern "C" fn fake_get_dlc_count(_: *mut ISteamApps) -> i32 {
    script(|s| i32::try_from(s.apps_extra.dlcs.len()).unwrap())
}

pub(super) unsafe extern "C" fn fake_get_dlc_data_by_index(
    _: *mut ISteamApps,
    index: i32,
    app: *mut u32,
    available: *mut bool,
    name: *mut c_char,
    capacity: i32,
) -> bool {
    let dlc = script(|s| {
        usize::try_from(index)
            .ok()
            .and_then(|i| s.apps_extra.dlcs.get(i).cloned())
    });
    let Some((id, is_available, text)) = dlc else {
        return false;
    };
    // SAFETY: the caller passes writable out-parameters.
    unsafe {
        app.write(id);
        available.write(is_available);
    }
    copy_out(
        "BGetDLCDataByIndex",
        &text,
        name,
        usize::try_from(capacity).unwrap(),
    );
    true
}

pub(super) unsafe extern "C" fn fake_is_dlc_installed(_: *mut ISteamApps, app: u32) -> bool {
    script(|s| s.apps_extra.installed_dlc.contains(&app))
}

pub(super) unsafe extern "C" fn fake_install_dlc(_: *mut ISteamApps, app: u32) {
    script(|s| s.apps_extra.dlc_changes.push((true, app)));
}

pub(super) unsafe extern "C" fn fake_uninstall_dlc(_: *mut ISteamApps, app: u32) {
    script(|s| s.apps_extra.dlc_changes.push((false, app)));
}

pub(super) unsafe extern "C" fn fake_get_current_beta_name(
    _: *mut ISteamApps,
    name: *mut c_char,
    capacity: i32,
) -> bool {
    let Some(text) = script(|s| s.apps_extra.current_beta.clone()) else {
        return false;
    };
    copy_out(
        "GetCurrentBetaName",
        &text,
        name,
        usize::try_from(capacity).unwrap(),
    );
    true
}

pub(super) unsafe extern "C" fn fake_get_num_betas(
    _: *mut ISteamApps,
    available: *mut i32,
    private: *mut i32,
) -> i32 {
    script(|s| {
        // SAFETY: the caller passes writable out-parameters.
        unsafe {
            available.write(s.apps_extra.available_betas);
            private.write(s.apps_extra.private_betas);
        }
        i32::try_from(s.apps_extra.betas.len()).unwrap()
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe extern "C" fn fake_get_beta_info(
    _: *mut ISteamApps,
    index: i32,
    flags: *mut u32,
    build: *mut u32,
    name: *mut c_char,
    name_capacity: i32,
    description: *mut c_char,
    description_capacity: i32,
    updated: *mut u32,
) -> bool {
    let beta = script(|s| {
        usize::try_from(index)
            .ok()
            .and_then(|i| s.apps_extra.betas.get(i).cloned())
    });
    let Some(beta) = beta else {
        return false;
    };
    // SAFETY: the caller passes writable out-parameters.
    unsafe {
        flags.write(beta.flags);
        build.write(beta.build);
        updated.write(beta.updated);
    }
    copy_out(
        "GetBetaInfo",
        &beta.name,
        name,
        usize::try_from(name_capacity).unwrap(),
    );
    copy_out(
        "GetBetaInfo",
        &beta.description,
        description,
        usize::try_from(description_capacity).unwrap(),
    );
    true
}

pub(super) unsafe extern "C" fn fake_set_active_beta(
    _: *mut ISteamApps,
    name: *const c_char,
) -> bool {
    // SAFETY: the caller passes a NUL-terminated string.
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    script(|s| {
        s.apps_extra.active_beta = Some(name);
        !s.refuse
    })
}

pub(super) unsafe extern "C" fn fake_mark_content_corrupt(
    _: *mut ISteamApps,
    missing_only: bool,
) -> bool {
    script(|s| {
        s.apps_extra.corrupt.push(missing_only);
        !s.refuse
    })
}

/// Answers the directory's length, as the header says, whatever fit.
pub(super) unsafe extern "C" fn fake_get_app_install_dir(
    _: *mut ISteamApps,
    _: u32,
    folder: *mut c_char,
    capacity: u32,
) -> u32 {
    let dir = script(|s| s.apps_extra.install_dir.clone());
    copy_out(
        "GetAppInstallDir",
        &dir,
        folder,
        usize::try_from(capacity).unwrap(),
    );
    u32::try_from(dir.len()).unwrap()
}

pub(super) unsafe extern "C" fn fake_get_file_details(
    _: *mut ISteamApps,
    name: *const c_char,
) -> SteamApiCall {
    // SAFETY: the caller passes a NUL-terminated string.
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    script(|s| {
        s.apps_extra.file_asked = Some(name);
        s.apps_extra.file_call
    })
}

pub(super) const REMOTE_PLAY: RemotePlayFns = RemotePlayFns {
    accessor: remote_play_accessor,
    get_session_count: fake_session_count,
    get_session_id: fake_session_id,
    session_remote_play_together: fake_together,
    get_session_steam_id: fake_session_user,
    get_session_guest_id: fake_session_guest,
    get_session_client_name: fake_session_name,
    get_session_client_form_factor: fake_session_form_factor,
    get_session_client_resolution: fake_session_resolution,
    show_remote_play_together_ui: fake_show_panel,
    send_remote_play_together_invite: fake_invite,
};

unsafe extern "C" fn remote_play_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::REMOTE_PLAY.accessor)
}

/// The scripted session with `id`, if any.
fn session(id: u32) -> Option<FakeSession> {
    script(|s| {
        s.remote_play
            .sessions
            .iter()
            .find(|session| session.id == id)
            .cloned()
    })
}

unsafe extern "C" fn fake_session_count(_: *mut ISteamRemotePlay) -> u32 {
    script(|s| u32::try_from(s.remote_play.sessions.len()).unwrap())
}

unsafe extern "C" fn fake_session_id(_: *mut ISteamRemotePlay, index: i32) -> u32 {
    script(|s| {
        usize::try_from(index)
            .ok()
            .and_then(|i| s.remote_play.sessions.get(i))
            .map_or(0, |session| session.id)
    })
}

unsafe extern "C" fn fake_together(_: *mut ISteamRemotePlay, id: u32) -> bool {
    session(id).is_some_and(|session| session.together)
}

unsafe extern "C" fn fake_session_user(_: *mut ISteamRemotePlay, id: u32) -> u64 {
    session(id).map_or(0, |session| session.user)
}

unsafe extern "C" fn fake_session_guest(_: *mut ISteamRemotePlay, id: u32) -> u32 {
    session(id).map_or(0, |session| session.guest)
}

unsafe extern "C" fn fake_session_name(_: *mut ISteamRemotePlay, id: u32) -> *const c_char {
    session(id)
        .and_then(|session| session.name)
        .map_or(core::ptr::null(), CStr::as_ptr)
}

unsafe extern "C" fn fake_session_form_factor(_: *mut ISteamRemotePlay, id: u32) -> i32 {
    session(id).map_or(0, |session| session.form_factor)
}

unsafe extern "C" fn fake_session_resolution(
    _: *mut ISteamRemotePlay,
    id: u32,
    width: *mut i32,
    height: *mut i32,
) -> bool {
    let Some((w, h)) = session(id).and_then(|session| session.resolution) else {
        return false;
    };
    // SAFETY: the caller passes writable out-parameters.
    unsafe {
        width.write(w);
        height.write(h);
    }
    true
}

unsafe extern "C" fn fake_show_panel(_: *mut ISteamRemotePlay) -> bool {
    script(|s| {
        s.remote_play.panels += 1;
        !s.refuse
    })
}

unsafe extern "C" fn fake_invite(_: *mut ISteamRemotePlay, friend: u64) -> bool {
    script(|s| {
        s.remote_play.invites.push(friend);
        !s.refuse
    })
}
