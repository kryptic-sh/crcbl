//! `ISteamApps` beyond the basics (Steamworks slice 11): who
//! owns what, the game's DLC, its beta branches, where it is installed, and
//! a file's details.
//!
//! Every string Steam writes into a buffer of this crate's is read through
//! [`grow`], which offers a larger buffer while the answer fills the one it
//! had — Steam's copies stop one byte short to leave a NUL, so a string that
//! reaches the last byte but one may have been cut — and reports
//! [`SteamError::Truncated`] rather than a cut string once the buffer reaches
//! [`MAX_TEXT_BYTES`].

use core::ffi::c_char;
use std::{ffi::CString, path::PathBuf};

use super::Apps;
use crate::{
    AppId, EResult, SteamCall, SteamError, SteamId,
    call::{CallRow, private::Answer},
    callbacks::{Base, fixed_string, read},
    client::Client,
    error::c_string,
    ffi::structs,
};

/// The first buffer a string read offers.
const FIRST_TEXT_BYTES: usize = 256;

/// The most a string read grows its buffer to. Far past any DLC name, beta
/// description or install path; a string still filling it is reported as
/// truncated.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

/// Calls `fill` with `N` buffers of one size, doubling the size while any
/// answer reaches its buffer's last byte but one, and answers what `fill`
/// returned with the buffers.
///
/// `fill` answers `None` for a call Steam refused, which is
/// [`SteamError::Refused`] naming `call`.
pub(crate) fn grow<const N: usize, R>(
    call: &'static str,
    mut fill: impl FnMut(&mut [Vec<u8>; N], i32) -> Option<R>,
) -> Result<(R, [Vec<u8>; N]), SteamError> {
    let mut size = FIRST_TEXT_BYTES;
    loop {
        let capacity = i32::try_from(size).map_err(|_| SteamError::Truncated(call))?;
        let mut buffers: [Vec<u8>; N] = std::array::from_fn(|_| vec![0; size]);
        let answer = fill(&mut buffers, capacity).ok_or(SteamError::Refused(call))?;
        let fits = buffers.iter().all(|buffer| {
            let text = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
            text + 1 < buffer.len()
        });
        if fits {
            return Ok((answer, buffers));
        }
        if size >= MAX_TEXT_BYTES {
            return Err(SteamError::Truncated(call));
        }
        size *= 2;
    }
}

/// One DLC of the running game (`BGetDLCDataByIndex`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dlc {
    /// Its app id.
    pub app: AppId,
    /// Whether it can be bought or installed — not whether the player owns
    /// it; that is [`Apps::dlc_installed`].
    pub available: bool,
    /// Its store name.
    pub name: String,
}

/// How many beta branches the game has (`GetNumBetas`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BetaCount {
    /// Every branch, the default `public` one included.
    pub total: u32,
    /// Those the player may select.
    pub available: u32,
    /// Those behind a password.
    pub private: u32,
}

/// What a beta branch is (`EBetaBranchFlags`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BetaFlags(pub u32);

impl BetaFlags {
    /// The default branch, `public` (`k_EBetaBranch_Default`).
    pub const DEFAULT: Self = Self(1);
    /// Selectable (`k_EBetaBranch_Available`).
    pub const AVAILABLE: Self = Self(2);
    /// Behind a password (`k_EBetaBranch_Private`).
    pub const PRIVATE: Self = Self(4);
    /// The one selected (`k_EBetaBranch_Selected`).
    pub const SELECTED: Self = Self(8);
    /// The one installed (`k_EBetaBranch_Installed`).
    pub const INSTALLED: Self = Self(16);

    /// Whether every flag in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// One beta branch (`GetBetaInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beta {
    /// Its name, as [`Apps::set_active_beta`] takes it.
    pub name: String,
    /// Its description.
    pub description: String,
    /// What it is.
    pub flags: BetaFlags,
    /// Its current build.
    pub build: u32,
    /// When it last changed, as a Unix time.
    pub updated: u32,
}

impl Apps<'_> {
    /// Whether the player owns `app` — a demo, a related game
    /// (`BIsSubscribedApp`).
    #[must_use]
    pub fn subscribed_app(&self, app: AppId) -> bool {
        let client = &self.steam.client;
        // SAFETY: `client.apps` is the non-null interface init resolved;
        // `Apps` borrows the `!Send` `Steam`, so this is the pump thread.
        unsafe { (client.lib.fns.apps.is_subscribed_app)(client.apps, app.0) }
    }

    /// Whether the player's copy is the low-violence one (`BIsLowViolence`).
    #[must_use]
    pub fn low_violence(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.is_low_violence)(client.apps) }
    }

    /// Whether the player is VAC-banned in this game (`BIsVACBanned`).
    #[must_use]
    pub fn vac_banned(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.is_vac_banned)(client.apps) }
    }

    /// When the player first bought `app`, as a Unix time; zero when they
    /// have not (`GetEarliestPurchaseUnixTime`).
    #[must_use]
    pub fn purchase_time(&self, app: AppId) -> u32 {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.get_earliest_purchase_unix_time)(client.apps, app.0) }
    }

    /// Whether the player plays through a free weekend
    /// (`BIsSubscribedFromFreeWeekend`).
    #[must_use]
    pub fn free_weekend(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.is_subscribed_from_free_weekend)(client.apps) }
    }

    /// Whether the player borrowed the game through Family Sharing
    /// (`BIsSubscribedFromFamilySharing`); [`owner`](Self::owner) is then
    /// the lender.
    #[must_use]
    pub fn family_shared(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.is_subscribed_from_family_sharing)(client.apps) }
    }

    /// Who owns the licence the game runs under (`GetAppOwner`).
    #[must_use]
    pub fn owner(&self) -> SteamId {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        SteamId(unsafe { (client.lib.fns.apps.get_app_owner)(client.apps) })
    }

    /// The build the game runs, which can change under it
    /// (`GetAppBuildId`).
    #[must_use]
    pub fn build_id(&self) -> i32 {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.get_app_build_id)(client.apps) }
    }

    /// Whether `app` is installed, owned or not (`BIsAppInstalled`).
    #[must_use]
    pub fn installed(&self, app: AppId) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.is_app_installed)(client.apps, app.0) }
    }

    /// Where `app` is installed, or `None` when it is not
    /// (`GetAppInstallDir`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Truncated`] for a path past [`MAX_TEXT_BYTES`].
    pub fn install_dir(&self, app: AppId) -> Result<Option<PathBuf>, SteamError> {
        let client = &self.steam.client;
        let (length, [buffer]) = grow("GetAppInstallDir", |[buffer], capacity| {
            let capacity = u32::try_from(capacity).ok()?;
            // SAFETY: as in `subscribed_app`; `buffer` is `capacity` writable
            // bytes.
            Some(unsafe {
                (client.lib.fns.apps.get_app_install_dir)(
                    client.apps,
                    app.0,
                    buffer.as_mut_ptr().cast::<c_char>(),
                    capacity,
                )
            })
        })?;
        if length == 0 {
            return Ok(None);
        }
        let (path, lossy) = fixed_string(&buffer);
        self.count_lossy(lossy);
        Ok(Some(PathBuf::from(path)))
    }

    /// How many DLC the game has (`GetDLCCount`).
    #[must_use]
    pub fn dlc_count(&self) -> u32 {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        let count = unsafe { (client.lib.fns.apps.get_dlc_count)(client.apps) };
        u32::try_from(count).unwrap_or(0)
    }

    /// The DLC at `index`, below [`dlc_count`](Self::dlc_count)
    /// (`BGetDLCDataByIndex`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] for an index Steam has no DLC at;
    /// [`SteamError::Truncated`] for a name past [`MAX_TEXT_BYTES`].
    pub fn dlc(&self, index: u32) -> Result<Dlc, SteamError> {
        let index = i32::try_from(index).map_err(|_| SteamError::Refused("BGetDLCDataByIndex"))?;
        let client = &self.steam.client;
        let ((app, available), [name]) = grow("BGetDLCDataByIndex", |[name], capacity| {
            let mut app = 0_u32;
            let mut available = false;
            // SAFETY: as in `subscribed_app`; both out-parameters are writable
            // and `name` is `capacity` writable bytes.
            let found = unsafe {
                (client.lib.fns.apps.get_dlc_data_by_index)(
                    client.apps,
                    index,
                    &raw mut app,
                    &raw mut available,
                    name.as_mut_ptr().cast::<c_char>(),
                    capacity,
                )
            };
            found.then_some((app, available))
        })?;
        let (name, lossy) = fixed_string(&name);
        self.count_lossy(lossy);
        Ok(Dlc {
            app: AppId(app),
            available,
            name,
        })
    }

    /// Whether the player owns DLC `app` and has it installed
    /// (`BIsDlcInstalled`).
    #[must_use]
    pub fn dlc_installed(&self, app: AppId) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.is_dlc_installed)(client.apps, app.0) }
    }

    /// Installs optional DLC `app` (`InstallDLC`);
    /// [`SteamEvent::DlcInstalled`](crate::SteamEvent::DlcInstalled) follows.
    pub fn install_dlc(&self, app: AppId) {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.install_dlc)(client.apps, app.0) };
    }

    /// Uninstalls optional DLC `app` (`UninstallDLC`).
    pub fn uninstall_dlc(&self, app: AppId) {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        unsafe { (client.lib.fns.apps.uninstall_dlc)(client.apps, app.0) };
    }

    /// The branch the game runs from — `public` by default
    /// (`GetCurrentBetaName`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam answers `false`;
    /// [`SteamError::Truncated`] for a name past [`MAX_TEXT_BYTES`].
    pub fn current_beta(&self) -> Result<String, SteamError> {
        let client = &self.steam.client;
        let ((), [name]) = grow("GetCurrentBetaName", |[name], capacity| {
            // SAFETY: as in `subscribed_app`; `name` is `capacity` writable
            // bytes.
            let read = unsafe {
                (client.lib.fns.apps.get_current_beta_name)(
                    client.apps,
                    name.as_mut_ptr().cast::<c_char>(),
                    capacity,
                )
            };
            read.then_some(())
        })?;
        let (name, lossy) = fixed_string(&name);
        self.count_lossy(lossy);
        Ok(name)
    }

    /// How many branches there are (`GetNumBetas`).
    #[must_use]
    pub fn beta_count(&self) -> BetaCount {
        let client = &self.steam.client;
        let mut available = 0_i32;
        let mut private = 0_i32;
        // SAFETY: as in `subscribed_app`; both out-parameters are writable.
        let total = unsafe {
            (client.lib.fns.apps.get_num_betas)(client.apps, &raw mut available, &raw mut private)
        };
        let count = |n: i32| u32::try_from(n).unwrap_or(0);
        BetaCount {
            total: count(total),
            available: count(available),
            private: count(private),
        }
    }

    /// The branch at `index`, below [`BetaCount::total`] (`GetBetaInfo`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] for an index Steam has no branch at;
    /// [`SteamError::Truncated`] for a name or description past
    /// [`MAX_TEXT_BYTES`].
    pub fn beta(&self, index: u32) -> Result<Beta, SteamError> {
        let index = i32::try_from(index).map_err(|_| SteamError::Refused("GetBetaInfo"))?;
        let client = &self.steam.client;
        let ((flags, build, updated), [name, description]) =
            grow("GetBetaInfo", |[name, description], capacity| {
                let (mut flags, mut build, mut updated) = (0_u32, 0_u32, 0_u32);
                // SAFETY: as in `subscribed_app`; every out-parameter is
                // writable, and each buffer is `capacity` writable bytes.
                let found = unsafe {
                    (client.lib.fns.apps.get_beta_info)(
                        client.apps,
                        index,
                        &raw mut flags,
                        &raw mut build,
                        name.as_mut_ptr().cast::<c_char>(),
                        capacity,
                        description.as_mut_ptr().cast::<c_char>(),
                        capacity,
                        &raw mut updated,
                    )
                };
                found.then_some((flags, build, updated))
            })?;
        let (name, lossy_name) = fixed_string(&name);
        let (description, lossy_description) = fixed_string(&description);
        self.count_lossy(lossy_name);
        self.count_lossy(lossy_description);
        Ok(Beta {
            name,
            description,
            flags: BetaFlags(flags),
            build,
            updated,
        })
    }

    /// Selects branch `name`; the game may need a restart for Steam to update
    /// to it (`SetActiveBeta`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`]; [`SteamError::Refused`] when Steam
    /// answers `false`.
    pub fn set_active_beta(&self, name: &str) -> Result<(), SteamError> {
        let name = c_string(name, "beta name", usize::MAX)?;
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`; `name` is NUL-terminated for the
        // call.
        let set = unsafe { (client.lib.fns.apps.set_active_beta)(client.apps, name.as_ptr()) };
        set.then_some(())
            .ok_or(SteamError::Refused("SetActiveBeta"))
    }

    /// Tells Steam the game's files look corrupt — or, with `missing_only`,
    /// only missing — so it verifies them (`MarkContentCorrupt`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam answers `false`.
    pub fn mark_content_corrupt(&self, missing_only: bool) -> Result<(), SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed_app`.
        let marked =
            unsafe { (client.lib.fns.apps.mark_content_corrupt)(client.apps, missing_only) };
        marked
            .then_some(())
            .ok_or(SteamError::Refused("MarkContentCorrupt"))
    }

    fn count_lossy(&self, lossy: bool) {
        if lossy {
            self.steam
                .lossy_strings
                .set(self.steam.lossy_strings.get() + 1);
        }
    }
}

impl crate::Steam {
    /// Asks for the size and SHA-1 Steam has for one of the game's depot
    /// files (`GetFileDetails`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`]; [`SteamError::Refused`] when Steam did
    /// not start the call.
    pub fn file_details(&mut self, file: &str) -> Result<SteamCall<FileDetails>, SteamError> {
        let file: CString = c_string(file, "file name", usize::MAX)?;
        let client = &self.client;
        // SAFETY: `client.apps` is live; `Steam` is `!Send`, so this is the
        // pump thread; `file` is NUL-terminated for the call.
        let handle = unsafe { (client.lib.fns.apps.get_file_details)(client.apps, file.as_ptr()) };
        self.calls
            .register(handle)
            .ok_or(SteamError::Refused("GetFileDetails"))
    }
}

/// The answer to [`Steam::file_details`](crate::Steam::file_details)
/// (`FileDetailsResult_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileDetails {
    /// `EResult::OK`, or why Steam has no details.
    pub result: EResult,
    /// The file's original size in bytes.
    pub size: u64,
    /// The file's original SHA-1.
    pub sha1: [u8; 20],
    /// Steam's flags for it.
    pub flags: u32,
}

impl Answer for FileDetails {
    const ROW: CallRow = CallRow {
        base: Base::Apps,
        offset: 23,
        #[cfg(test)]
        name: "FileDetailsResult_t",
        size: size_of::<structs::FileDetailsResult>(),
    };

    fn build(bytes: &[u8], _: &mut crate::Steam) -> Option<Self> {
        let raw = read::<structs::FileDetailsResult>(bytes)?;
        Some(Self {
            result: EResult(raw.result),
            size: raw.size,
            sha1: raw.sha1,
            flags: raw.flags,
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

#[cfg(test)]
mod tests;
