//! `ISteamUserStats`: the local player's stats and achievements.
//!
//! Since SDK 1.61 Steam fetches them on its own after init — there is no
//! `RequestCurrentStats` — and says so with `UserStatsReceived_t`, which the
//! pump turns into [`SteamEvent::StatsReceived`](crate::SteamEvent). Until the
//! local user's have arrived, every call here is
//! [`SteamError::StatsNotReady`]: Steam would answer `false`, which reads the
//! same as a misspelt name. Changes are local until [`Stats::store`], whose
//! answer is [`SteamEvent::StatsStored`](crate::SteamEvent); an unlock shows
//! Steam's toast then.

use std::ffi::CString;

use crate::{Steam, error::SteamError, matchmaking::refused_unless};

/// The longest stat or achievement name, in bytes: `k_cchStatNameMax`
/// (`isteamuserstats.h`) sizes the `char` arrays names travel in, the NUL
/// included.
pub const MAX_STAT_NAME_LENGTH: usize = 127;

/// An achievement's state (`GetAchievementAndUnlockTime`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Achieved {
    /// Whether it is unlocked.
    pub unlocked: bool,
    /// When, in seconds since the Unix epoch; `None` while locked, or when
    /// Steam does not know.
    pub unlock_time: Option<u32>,
}

/// `ISteamUserStats`, borrowed from a [`Steam`]; from [`Steam::stats`].
#[derive(Debug, Clone, Copy)]
pub struct Stats<'a> {
    steam: &'a Steam,
}

impl Steam {
    /// The local player's stats and achievements.
    #[must_use]
    pub fn stats(&self) -> Stats<'_> {
        Stats { steam: self }
    }
}

impl Stats<'_> {
    /// Whether the local user's stats have arrived, so the calls below can
    /// succeed.
    #[must_use]
    pub const fn ready(&self) -> bool {
        self.steam.stats_ready
    }

    /// An achievement's state.
    ///
    /// # Errors
    ///
    /// [`SteamError::StatsNotReady`]; [`SteamError::InteriorNul`] or
    /// [`SteamError::TooLong`] for a name Steam cannot be handed;
    /// [`SteamError::Refused`] for one the app does not define.
    pub fn achievement(&self, name: &str) -> Result<Achieved, SteamError> {
        let name = self.name(name)?;
        let client = &self.steam.client;
        let mut unlocked = false;
        let mut time = 0_u32;
        // SAFETY: `client.user_stats` is the non-null interface init resolved,
        // `Steam` is `!Send` so this is the pump thread, `name` is
        // NUL-terminated, and both out-parameters are writable.
        let known = unsafe {
            (client.lib.fns.user_stats.get_achievement_and_unlock_time)(
                client.user_stats,
                name.as_ptr(),
                &raw mut unlocked,
                &raw mut time,
            )
        };
        if !known {
            return Err(SteamError::Refused("GetAchievementAndUnlockTime"));
        }
        Ok(Achieved {
            unlocked,
            unlock_time: (unlocked && time != 0).then_some(time),
        })
    }

    /// Unlocks an achievement, locally until [`store`](Self::store).
    ///
    /// # Errors
    ///
    /// As [`achievement`](Self::achievement).
    pub fn set_achievement(&self, name: &str) -> Result<(), SteamError> {
        let name = self.name(name)?;
        let client = &self.steam.client;
        // SAFETY: as in `achievement`.
        let done = unsafe {
            (client.lib.fns.user_stats.set_achievement)(client.user_stats, name.as_ptr())
        };
        refused_unless(done, "SetAchievement")
    }

    /// Locks an achievement again, locally until [`store`](Self::store) — for
    /// tests and tools; a shipped game rarely takes one back.
    ///
    /// # Errors
    ///
    /// As [`achievement`](Self::achievement).
    pub fn clear_achievement(&self, name: &str) -> Result<(), SteamError> {
        let name = self.name(name)?;
        let client = &self.steam.client;
        // SAFETY: as in `achievement`.
        let done = unsafe {
            (client.lib.fns.user_stats.clear_achievement)(client.user_stats, name.as_ptr())
        };
        refused_unless(done, "ClearAchievement")
    }

    /// An integer stat.
    ///
    /// # Errors
    ///
    /// As [`achievement`](Self::achievement); a float stat read as an integer
    /// is refused too.
    pub fn i32(&self, name: &str) -> Result<i32, SteamError> {
        let name = self.name(name)?;
        let client = &self.steam.client;
        let mut value = 0_i32;
        // SAFETY: as in `achievement`; `value` is writable.
        let known = unsafe {
            (client.lib.fns.user_stats.get_stat_i32)(
                client.user_stats,
                name.as_ptr(),
                &raw mut value,
            )
        };
        refused_unless(known, "GetStatInt32").map(|()| value)
    }

    /// A float stat.
    ///
    /// # Errors
    ///
    /// As [`i32`](Self::i32).
    pub fn f32(&self, name: &str) -> Result<f32, SteamError> {
        let name = self.name(name)?;
        let client = &self.steam.client;
        let mut value = 0.0_f32;
        // SAFETY: as in `achievement`; `value` is writable.
        let known = unsafe {
            (client.lib.fns.user_stats.get_stat_f32)(
                client.user_stats,
                name.as_ptr(),
                &raw mut value,
            )
        };
        refused_unless(known, "GetStatFloat").map(|()| value)
    }

    /// Sets an integer stat, locally until [`store`](Self::store). Steam
    /// refuses a value the stat's settings forbid (below its minimum, or
    /// lower than before for an increment-only stat).
    ///
    /// # Errors
    ///
    /// As [`i32`](Self::i32).
    pub fn set_i32(&self, name: &str, value: i32) -> Result<(), SteamError> {
        let name = self.name(name)?;
        let client = &self.steam.client;
        // SAFETY: as in `achievement`.
        let done = unsafe {
            (client.lib.fns.user_stats.set_stat_i32)(client.user_stats, name.as_ptr(), value)
        };
        refused_unless(done, "SetStatInt32")
    }

    /// Sets a float stat, locally until [`store`](Self::store).
    ///
    /// # Errors
    ///
    /// As [`i32`](Self::i32).
    pub fn set_f32(&self, name: &str, value: f32) -> Result<(), SteamError> {
        let name = self.name(name)?;
        let client = &self.steam.client;
        // SAFETY: as in `achievement`.
        let done = unsafe {
            (client.lib.fns.user_stats.set_stat_f32)(client.user_stats, name.as_ptr(), value)
        };
        refused_unless(done, "SetStatFloat")
    }

    /// Sends every change to Steam's servers (`StoreStats`); the answer is a
    /// later [`SteamEvent::StatsStored`](crate::SteamEvent), and an unlock's
    /// toast appears then.
    ///
    /// # Errors
    ///
    /// [`SteamError::StatsNotReady`], or [`SteamError::Refused`] when Steam
    /// would not start the store.
    pub fn store(&self) -> Result<(), SteamError> {
        self.check_ready()?;
        let client = &self.steam.client;
        // SAFETY: as in `achievement`.
        let started = unsafe { (client.lib.fns.user_stats.store_stats)(client.user_stats) };
        refused_unless(started, "StoreStats")
    }

    fn check_ready(&self) -> Result<(), SteamError> {
        if self.ready() {
            Ok(())
        } else {
            Err(SteamError::StatsNotReady)
        }
    }

    /// A name, once the stats are ready, as Steam takes it.
    fn name(&self, name: &str) -> Result<CString, SteamError> {
        self.check_ready()?;
        crate::error::c_string(name, "stat or achievement name", MAX_STAT_NAME_LENGTH)
    }
}

#[cfg(test)]
mod tests;
