//! `ISteamUserStats` leaderboards: find or create one, upload a score,
//! download entries — each an asynchronous call redeemed with
//! [`Steam::take`].

use crate::{
    Steam, SteamCall, SteamId,
    call::{CallRow, private::Answer},
    callbacks::{Base, read},
    client::Client,
    error::SteamError,
    ffi::structs::{self, LeaderboardEntry, steam_id},
};

/// The longest leaderboard name, in bytes: `k_cchLeaderboardNameMax`
/// (`isteamuserstats.h`) is 128 "bytes for a leaderboard name", taken here to
/// include the NUL as the stat-name limit does.
pub const MAX_LEADERBOARD_NAME_LENGTH: usize = 127;

/// The most details one entry holds: `k_cLeaderboardDetailsMax`
/// (`isteamuserstats.h`).
pub const MAX_LEADERBOARD_DETAILS: usize = 64;

/// A leaderboard, found: its handle, good for this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Leaderboard(u64);

/// Which scores rank first (`ELeaderboardSortMethod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderboardSort {
    /// The lowest score is the top one — a race time.
    Ascending,
    /// The highest score is the top one.
    Descending,
}

/// How Steam shows a score (`ELeaderboardDisplayType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderboardDisplay {
    /// A number.
    Numeric,
    /// A time, in seconds.
    TimeSeconds,
    /// A time, in milliseconds.
    TimeMilliseconds,
}

/// What an upload does to an existing score (`ELeaderboardUploadScoreMethod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadMethod {
    /// Keep the better of the two, by the board's sort.
    KeepBest,
    /// Replace it whatever it was.
    ForceUpdate,
}

/// Which entries to download (`ELeaderboardDataRequest` and its range).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    /// Global ranks `first..=last`, counted from 1.
    Global {
        /// The first rank.
        first: i32,
        /// The last rank.
        last: i32,
    },
    /// The entries around the local user's: `before` ranks above (negative,
    /// as Steam takes it) to `after` below.
    AroundUser {
        /// How far above, as a negative offset.
        before: i32,
        /// How far below.
        after: i32,
    },
    /// Every friend's entry, and the local user's.
    Friends,
}

impl Range {
    /// `(ELeaderboardDataRequest, nRangeStart, nRangeEnd)`.
    const fn raw(self) -> (i32, i32, i32) {
        match self {
            Self::Global { first, last } => (0, first, last),
            Self::AroundUser { before, after } => (1, before, after),
            Self::Friends => (2, 0, 0),
        }
    }
}

/// `ISteamUserStats`'s leaderboards, borrowed from a [`Steam`]; from
/// [`Steam::leaderboards`].
#[derive(Debug)]
pub struct Leaderboards<'a> {
    steam: &'a mut Steam,
}

impl Steam {
    /// Leaderboards: find, upload, download.
    pub fn leaderboards(&mut self) -> Leaderboards<'_> {
        Leaderboards { steam: self }
    }
}

impl Leaderboards<'_> {
    /// Finds a leaderboard, creating it with `sort` and `display` if the app
    /// has none by that name (`FindOrCreateLeaderboard`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::TooLong`] for a name
    /// Steam cannot be handed; [`SteamError::Refused`] when Steam did not
    /// start the call.
    pub fn find_or_create(
        &mut self,
        name: &str,
        sort: LeaderboardSort,
        display: LeaderboardDisplay,
    ) -> Result<SteamCall<LeaderboardFound>, SteamError> {
        let name = leaderboard_name(name)?;
        let sort = match sort {
            LeaderboardSort::Ascending => 1,
            LeaderboardSort::Descending => 2,
        };
        let display = match display {
            LeaderboardDisplay::Numeric => 1,
            LeaderboardDisplay::TimeSeconds => 2,
            LeaderboardDisplay::TimeMilliseconds => 3,
        };
        let client = &self.steam.client;
        // SAFETY: `client.user_stats` is the non-null interface init resolved,
        // `Steam` is `!Send` so this is the pump thread, and `name` is
        // NUL-terminated for the call.
        let handle = unsafe {
            (client.lib.fns.user_stats.find_or_create_leaderboard)(
                client.user_stats,
                name.as_ptr(),
                sort,
                display,
            )
        };
        self.register(handle, "FindOrCreateLeaderboard")
    }

    /// Finds a leaderboard the app already has (`FindLeaderboard`).
    ///
    /// # Errors
    ///
    /// As [`find_or_create`](Self::find_or_create).
    pub fn find(&mut self, name: &str) -> Result<SteamCall<LeaderboardFound>, SteamError> {
        let name = leaderboard_name(name)?;
        let client = &self.steam.client;
        // SAFETY: as in `find_or_create`.
        let handle = unsafe {
            (client.lib.fns.user_stats.find_leaderboard)(client.user_stats, name.as_ptr())
        };
        self.register(handle, "FindLeaderboard")
    }

    /// Uploads the local user's `score`, with up to
    /// [`MAX_LEADERBOARD_DETAILS`] game-defined `details`
    /// (`UploadLeaderboardScore`).
    ///
    /// # Errors
    ///
    /// [`SteamError::TooMany`] for more details than an entry holds, before
    /// the call; [`SteamError::Refused`] when Steam did not start it.
    pub fn upload(
        &mut self,
        leaderboard: Leaderboard,
        method: UploadMethod,
        score: i32,
        details: &[i32],
    ) -> Result<SteamCall<ScoreUploaded>, SteamError> {
        let count = match i32::try_from(details.len()) {
            Ok(count) if details.len() <= MAX_LEADERBOARD_DETAILS => count,
            _ => {
                return Err(SteamError::TooMany {
                    what: "leaderboard details",
                    max: MAX_LEADERBOARD_DETAILS,
                });
            }
        };
        let method = match method {
            UploadMethod::KeepBest => 1,
            UploadMethod::ForceUpdate => 2,
        };
        let client = &self.steam.client;
        // SAFETY: as in `find_or_create`; `details` is `count` readable
        // `int32`s for the call.
        let handle = unsafe {
            (client.lib.fns.user_stats.upload_leaderboard_score)(
                client.user_stats,
                leaderboard.0,
                method,
                score,
                details.as_ptr(),
                count,
            )
        };
        self.register(handle, "UploadLeaderboardScore")
    }

    /// Downloads the entries `range` names (`DownloadLeaderboardEntries`); the
    /// answer carries them, details and all.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn download(
        &mut self,
        leaderboard: Leaderboard,
        range: Range,
    ) -> Result<SteamCall<Entries>, SteamError> {
        let (request, start, end) = range.raw();
        let client = &self.steam.client;
        // SAFETY: as in `find_or_create`.
        let handle = unsafe {
            (client.lib.fns.user_stats.download_leaderboard_entries)(
                client.user_stats,
                leaderboard.0,
                request,
                start,
                end,
            )
        };
        self.register(handle, "DownloadLeaderboardEntries")
    }

    fn register<T: crate::CallResult>(
        &mut self,
        handle: u64,
        call: &'static str,
    ) -> Result<SteamCall<T>, SteamError> {
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused(call))
    }
}

/// A name as Steam takes it.
fn leaderboard_name(name: &str) -> Result<std::ffi::CString, SteamError> {
    crate::error::c_string(name, "leaderboard name", MAX_LEADERBOARD_NAME_LENGTH)
}

/// The answer to [`Leaderboards::find_or_create`] and
/// [`Leaderboards::find`] (`LeaderboardFindResult_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderboardFound {
    leaderboard: Option<Leaderboard>,
}

impl LeaderboardFound {
    /// The leaderboard, or `None` when there is none by that name.
    #[must_use]
    pub const fn leaderboard(&self) -> Option<Leaderboard> {
        self.leaderboard
    }
}

impl Answer for LeaderboardFound {
    const ROW: CallRow = CallRow {
        base: Base::UserStats,
        offset: 4,
        #[cfg(test)]
        name: "LeaderboardFindResult_t",
        size: size_of::<structs::LeaderboardFindResult>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::LeaderboardFindResult>(bytes)?;
        let handle = raw.leaderboard;
        Some(Self {
            leaderboard: (raw.found != 0 && handle != 0).then_some(Leaderboard(handle)),
        })
    }

    /// Nothing to release: a leaderboard's handles are Steam's for the
    /// session.
    fn abandon(_: &[u8], _: &Client) {}
}

/// The answer to [`Leaderboards::upload`] (`LeaderboardScoreUploaded_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreUploaded {
    /// Whether Steam took the upload.
    pub success: bool,
    /// The score sent.
    pub score: i32,
    /// Whether the board changed — `false` when a kept best was better.
    pub changed: bool,
    /// The local user's global rank now.
    pub rank: i32,
    /// Their rank before; `None` for a first entry.
    pub previous_rank: Option<i32>,
}

impl Answer for ScoreUploaded {
    const ROW: CallRow = CallRow {
        base: Base::UserStats,
        offset: 6,
        #[cfg(test)]
        name: "LeaderboardScoreUploaded_t",
        size: size_of::<structs::LeaderboardScoreUploaded>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::LeaderboardScoreUploaded>(bytes)?;
        let previous = raw.rank_previous;
        Some(Self {
            success: raw.success != 0,
            score: raw.score,
            changed: raw.changed != 0,
            rank: raw.rank_new,
            previous_rank: (previous != 0).then_some(previous),
        })
    }

    /// Nothing to release: a leaderboard's handles are Steam's for the
    /// session.
    fn abandon(_: &[u8], _: &Client) {}
}

/// One downloaded entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Whose.
    pub user: SteamId,
    /// Global rank, from 1.
    pub rank: i32,
    /// The score.
    pub score: i32,
    /// The game-defined details uploaded with it.
    pub details: Vec<i32>,
}

/// The answer to [`Leaderboards::download`] (`LeaderboardScoresDownloaded_t`,
/// then `GetDownloadedLeaderboardEntry` per entry, read at the pump).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entries {
    /// The board.
    pub leaderboard: Leaderboard,
    /// The entries, as Steam ordered them. One Steam would not hand over is
    /// left out.
    pub entries: Vec<Entry>,
}

impl Answer for Entries {
    const ROW: CallRow = CallRow {
        base: Base::UserStats,
        offset: 5,
        #[cfg(test)]
        name: "LeaderboardScoresDownloaded_t",
        size: size_of::<structs::LeaderboardScoresDownloaded>(),
    };

    fn build(bytes: &[u8], steam: &mut Steam) -> Option<Self> {
        let raw = read::<structs::LeaderboardScoresDownloaded>(bytes)?;
        let client = &steam.client;
        let count = raw.count.max(0);
        let mut entries = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
        for index in 0..count {
            // SAFETY: an all-zero `LeaderboardEntry` is a valid value, being
            // integers and bytes.
            let mut entry: LeaderboardEntry = unsafe { core::mem::zeroed() };
            let mut details = [0_i32; MAX_LEADERBOARD_DETAILS];
            let capacity = i32::try_from(details.len()).unwrap_or(i32::MAX);
            // SAFETY: `client.user_stats` is the non-null interface; the pump
            // runs on the pump thread; `index` is below the count Steam gave;
            // `entry` is writable, and `details` is `capacity` writable
            // `int32`s.
            let read = unsafe {
                (client.lib.fns.user_stats.get_downloaded_leaderboard_entry)(
                    client.user_stats,
                    raw.entries,
                    index,
                    &raw mut entry,
                    details.as_mut_ptr(),
                    capacity,
                )
            };
            if !read {
                continue;
            }
            let held = usize::try_from(entry.details)
                .unwrap_or(0)
                .min(MAX_LEADERBOARD_DETAILS);
            entries.push(Entry {
                user: SteamId(steam_id(entry.user)),
                rank: entry.rank,
                score: entry.score,
                details: details[..held].to_vec(),
            });
        }
        Some(Self {
            leaderboard: Leaderboard(raw.leaderboard),
            entries,
        })
    }

    /// Nothing to release: a leaderboard's handles are Steam's for the
    /// session.
    fn abandon(_: &[u8], _: &Client) {}
}

#[cfg(test)]
mod tests;
