//! `ISteamUGC` (Steamworks slice 14): the Workshop — finding
//! items, the ones the player subscribes to and where they are installed, and
//! making and updating items of the player's own.
//!
//! - **Queries** ([`Workshop::query_all`], [`Workshop::query_user`],
//!   [`Workshop::query_details`]) make a [`UgcQuery`], which owns Steam's
//!   query handle and releases it (`ReleaseQueryUGCRequest`) exactly once,
//!   when dropped — sent or not. [`Workshop::send`] asks Steam for the page;
//!   its answer, a [`QueryPage`], is read through the query that asked for it
//!   ([`Workshop::results`]), so the query outlives its page. A page holds at
//!   most [`UGC_RESULTS_PER_PAGE`] items, and [`QueryPage::next_page`] says
//!   whether another follows.
//! - **Subscriptions and installs**: [`Workshop::subscribe`] and
//!   [`Workshop::unsubscribe`] answer later; [`Workshop::subscribed_items`],
//!   [`Workshop::state`], [`Workshop::install_info`] and
//!   [`Workshop::download_progress`] answer now; an install or update arrives
//!   as [`SteamEvent::WorkshopItemInstalled`](crate::SteamEvent::WorkshopItemInstalled),
//!   and a download [`Workshop::download`] started finishes as
//!   [`SteamEvent::WorkshopItemDownloaded`](crate::SteamEvent::WorkshopItemDownloaded).
//! - **Making an item**: [`Workshop::create_item`] answers an [`ItemId`];
//!   [`Workshop::start_update`] opens an [`ItemUpdate`] on it, whose setters
//!   stage changes; [`Workshop::submit`] consumes the update — nothing can be
//!   staged on it afterwards — and hands back the call and a [`Submission`]
//!   that reports the upload's progress. [`Workshop::delete_item`] removes an
//!   item.
//!
//! Every length Steam's headers bound is checked before the call
//! ([`MAX_ITEM_TITLE_LENGTH`] and the rest), and the install folder is read
//! through the buffer growth every string read in this crate uses. Not bound,
//! each on demand: cursor queries, the query's other filters and per-item
//! extras (previews, key-value tags, children, statistics), votes, favourites,
//! dependencies, playtime tracking, the Workshop EULA, and the item's
//! additional previews and key-value tags on update.

mod query;
mod update;

pub use query::{ItemDetails, QueryPage, UgcQuery};
pub use update::{ItemUpdate, Submission, UpdateProgress, UpdateStatus};

use std::{path::PathBuf, sync::Arc};

use core::ffi::c_char;

use crate::{
    EResult, Steam, SteamCall, SteamError, SteamId,
    apps::grow,
    call::{CallRow, private::Answer},
    callbacks::{Base, fixed_string, read},
    client::Client,
    error::c_string,
    ffi::structs,
};

/// The most items one query page holds: `kNumUGCResultsPerPage`
/// (`isteamugc.h`).
pub const UGC_RESULTS_PER_PAGE: u32 = 50;

/// The longest item title, in bytes: `k_cchPublishedDocumentTitleMax`
/// (`isteamremotestorage.h`, `128 + 1`) less its NUL.
pub const MAX_ITEM_TITLE_LENGTH: usize = 128;

/// The longest item description, in bytes:
/// `k_cchPublishedDocumentDescriptionMax` (`isteamremotestorage.h`), taken
/// to include the NUL as the title's limit does.
pub const MAX_ITEM_DESCRIPTION_LENGTH: usize = 7999;

/// The longest change note on a submitted update, in bytes:
/// `k_cchPublishedDocumentChangeDescriptionMax` (`isteamremotestorage.h`),
/// less its NUL.
pub const MAX_CHANGE_NOTE_LENGTH: usize = 7999;

/// The longest developer metadata on an item, in bytes:
/// `k_cchDeveloperMetadataMax` (`isteamugc.h`), less its NUL.
pub const MAX_ITEM_METADATA_LENGTH: usize = 9999;

/// The longest single tag, in bytes. From Valve's `SetItemTags`
/// documentation ("each tag must be limited to 255 characters"), not a
/// header constant, so the drift gate cannot check it.
pub const MAX_ITEM_TAG_LENGTH: usize = 255;

/// `k_UGCQueryHandleInvalid` and `k_UGCUpdateHandleInvalid`: all ones.
const INVALID_HANDLE: u64 = u64::MAX;

/// A Workshop item (`PublishedFileId_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub u64);

/// How a query over every item ranks them (`EUGCQuery`). A newtype, so every
/// value the SDK names — and any it adds — can be passed; the common ones are
/// named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryOrder(pub i32);

impl QueryOrder {
    /// `k_EUGCQuery_RankedByVote`.
    pub const RANKED_BY_VOTE: Self = Self(0);
    /// Newest first (`k_EUGCQuery_RankedByPublicationDate`).
    pub const BY_PUBLICATION_DATE: Self = Self(1);
    /// `k_EUGCQuery_RankedByTrend`.
    pub const RANKED_BY_TREND: Self = Self(3);
    /// `k_EUGCQuery_FavoritedByFriendsRankedByPublicationDate`.
    pub const FAVORITED_BY_FRIENDS: Self = Self(4);
    /// `k_EUGCQuery_CreatedByFriendsRankedByPublicationDate`.
    pub const CREATED_BY_FRIENDS: Self = Self(5);
    /// `k_EUGCQuery_RankedByTextSearch`, with [`UgcQuery::search_text`].
    pub const RANKED_BY_TEXT_SEARCH: Self = Self(11);
    /// `k_EUGCQuery_RankedByTotalUniqueSubscriptions`.
    pub const RANKED_BY_UNIQUE_SUBSCRIPTIONS: Self = Self(12);
    /// `k_EUGCQuery_RankedByLastUpdatedDate`.
    pub const BY_LAST_UPDATED: Self = Self(19);
}

/// Which kinds of item a query matches (`EUGCMatchingUGCType`); the common
/// ones are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MatchingType(pub i32);

impl MatchingType {
    /// Every item, ready to use or for sale (`k_EUGCMatchingUGCType_Items`).
    pub const ITEMS: Self = Self(0);
    /// `k_EUGCMatchingUGCType_Items_ReadyToUse`.
    pub const ITEMS_READY_TO_USE: Self = Self(2);
    /// `k_EUGCMatchingUGCType_Collections`.
    pub const COLLECTIONS: Self = Self(3);
    /// `k_EUGCMatchingUGCType_Artwork`.
    pub const ARTWORK: Self = Self(4);
    /// `k_EUGCMatchingUGCType_Videos`.
    pub const VIDEOS: Self = Self(5);
    /// `k_EUGCMatchingUGCType_Screenshots`.
    pub const SCREENSHOTS: Self = Self(6);
    /// Ready-to-use items and in-game guides
    /// (`k_EUGCMatchingUGCType_UsableInGame`).
    pub const USABLE_IN_GAME: Self = Self(10);
    /// `k_EUGCMatchingUGCType_GameManagedItems`.
    pub const GAME_MANAGED_ITEMS: Self = Self(12);
    /// Everything (`k_EUGCMatchingUGCType_All`, `~0`) — which Steam accepts
    /// only for [`Workshop::query_user`].
    pub const ALL: Self = Self(-1);
}

/// One of a user's Workshop lists (`EUserUGCList`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserList {
    /// What they published.
    Published,
    /// What they voted on.
    VotedOn,
    /// What they voted up.
    VotedUp,
    /// What they voted down.
    VotedDown,
    /// What they marked to vote on later.
    WillVoteLater,
    /// What they favourited.
    Favorited,
    /// What they subscribe to.
    Subscribed,
    /// What they used or played.
    UsedOrPlayed,
    /// What they follow.
    Followed,
}

impl UserList {
    const fn raw(self) -> i32 {
        match self {
            Self::Published => 0,
            Self::VotedOn => 1,
            Self::VotedUp => 2,
            Self::VotedDown => 3,
            Self::WillVoteLater => 4,
            Self::Favorited => 5,
            Self::Subscribed => 6,
            Self::UsedOrPlayed => 7,
            Self::Followed => 8,
        }
    }
}

/// How a user's list is sorted (`EUserUGCListSortOrder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserListOrder {
    /// Newest first — Steam's default.
    CreationOrderDesc,
    /// Oldest first.
    CreationOrderAsc,
    /// By title.
    TitleAsc,
    /// Most recently updated first.
    LastUpdatedDesc,
    /// Most recently subscribed first.
    SubscriptionDateDesc,
    /// Best voted first.
    VoteScoreDesc,
    /// For moderation.
    ForModeration,
}

impl UserListOrder {
    const fn raw(self) -> i32 {
        match self {
            Self::CreationOrderDesc => 0,
            Self::CreationOrderAsc => 1,
            Self::TitleAsc => 2,
            Self::LastUpdatedDesc => 3,
            Self::SubscriptionDateDesc => 4,
            Self::VoteScoreDesc => 5,
            Self::ForModeration => 6,
        }
    }
}

/// What kind of item (`EWorkshopFileType`); the kinds a game makes are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileType(pub i32);

impl FileType {
    /// An ordinary item players subscribe to (`k_EWorkshopFileTypeCommunity`).
    pub const COMMUNITY: Self = Self(0);
    /// Meant to be voted on for sale in game
    /// (`k_EWorkshopFileTypeMicrotransaction`).
    pub const MICROTRANSACTION: Self = Self(1);
    /// `k_EWorkshopFileTypeCollection`.
    pub const COLLECTION: Self = Self(2);
    /// `k_EWorkshopFileTypeArt`.
    pub const ART: Self = Self(3);
    /// `k_EWorkshopFileTypeVideo`.
    pub const VIDEO: Self = Self(4);
    /// `k_EWorkshopFileTypeScreenshot`.
    pub const SCREENSHOT: Self = Self(5);
    /// Managed by the game, not the player, and not shown on the web
    /// (`k_EWorkshopFileTypeGameManagedItem`).
    pub const GAME_MANAGED_ITEM: Self = Self(15);
}

/// Who can see an item (`ERemoteStoragePublishedFileVisibility`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// Everyone.
    Public,
    /// The owner's friends.
    FriendsOnly,
    /// The owner alone.
    Private,
    /// Anyone with the link; not listed.
    Unlisted,
    /// A value this crate does not name, kept.
    Other(i32),
}

impl Visibility {
    const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Public,
            1 => Self::FriendsOnly,
            2 => Self::Private,
            3 => Self::Unlisted,
            other => Self::Other(other),
        }
    }

    const fn raw(self) -> i32 {
        match self {
            Self::Public => 0,
            Self::FriendsOnly => 1,
            Self::Private => 2,
            Self::Unlisted => 3,
            Self::Other(raw) => raw,
        }
    }
}

/// What the client knows of an item (`EItemState`), a set of flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ItemState(pub u32);

impl ItemState {
    /// Not tracked on this client (`k_EItemStateNone`).
    pub const NONE: Self = Self(0);
    /// The player subscribes to it (`k_EItemStateSubscribed`).
    pub const SUBSCRIBED: Self = Self(1);
    /// Made with the old `ISteamRemoteStorage` (`k_EItemStateLegacyItem`).
    pub const LEGACY: Self = Self(2);
    /// Installed and usable, maybe out of date (`k_EItemStateInstalled`).
    pub const INSTALLED: Self = Self(4);
    /// Not installed yet, or its creator updated it
    /// (`k_EItemStateNeedsUpdate`).
    pub const NEEDS_UPDATE: Self = Self(8);
    /// Downloading now (`k_EItemStateDownloading`).
    pub const DOWNLOADING: Self = Self(16);
    /// [`Workshop::download`] was asked for it
    /// (`k_EItemStateDownloadPending`).
    pub const DOWNLOAD_PENDING: Self = Self(32);
    /// Disabled on this machine, so not to be treated as subscribed
    /// (`k_EItemStateDisabledLocally`).
    pub const DISABLED_LOCALLY: Self = Self(64);

    /// Whether every flag in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Where an installed item is (`GetItemInstallInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallInfo {
    /// Its size on disk, in bytes.
    pub size_on_disk: u64,
    /// Its folder.
    pub folder: PathBuf,
    /// When it was last updated, in Unix seconds.
    pub updated: u32,
}

/// How far a download is (`GetItemDownloadInfo`), in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    /// Downloaded so far.
    pub downloaded: u64,
    /// The whole download; zero until Steam knows.
    pub total: u64,
}

/// The Workshop, borrowed from a [`Steam`]; from [`Steam::workshop`].
#[derive(Debug)]
pub struct Workshop<'a> {
    steam: &'a mut Steam,
}

impl Steam {
    /// The Workshop: queries, subscriptions, installs and the player's own
    /// items.
    pub fn workshop(&mut self) -> Workshop<'_> {
        Workshop { steam: self }
    }
}

impl Workshop<'_> {
    /// A query over every item made for the running game, ranked by `order`
    /// and matching `matching`, for page `page` of the results
    /// (`CreateQueryAllUGCRequestPage`). Pages count from 1.
    ///
    /// # Errors
    ///
    /// [`SteamError::OutOfRange`] for page 0; [`SteamError::Refused`] when
    /// Steam makes no query.
    pub fn query_all(
        &self,
        order: QueryOrder,
        matching: MatchingType,
        page: u32,
    ) -> Result<UgcQuery, SteamError> {
        if page == 0 {
            return Err(SteamError::OutOfRange("page"));
        }
        let client = &self.steam.client;
        let app = self.steam.app.0;
        // SAFETY: `client.ugc` is live; `Workshop` borrows the `!Send`
        // `Steam`, so this is the pump thread; every argument is by value.
        let handle = unsafe {
            (client.lib.fns.ugc.create_query_all_ugc_request_page)(
                client.ugc, order.0, matching.0, app, app, page,
            )
        };
        UgcQuery::new(client, handle, "CreateQueryAllUGCRequestPage")
    }

    /// A query over one of `user`'s lists, for the running game, for page
    /// `page` (`CreateQueryUserUGCRequest`). Pages count from 1.
    ///
    /// # Errors
    ///
    /// As [`query_all`](Self::query_all).
    pub fn query_user(
        &self,
        user: SteamId,
        list: UserList,
        matching: MatchingType,
        order: UserListOrder,
        page: u32,
    ) -> Result<UgcQuery, SteamError> {
        if page == 0 {
            return Err(SteamError::OutOfRange("page"));
        }
        // `AccountID_t` is the Steam id's low 32 bits.
        let account = (user.0 & u64::from(u32::MAX)) as u32;
        let client = &self.steam.client;
        let app = self.steam.app.0;
        // SAFETY: as in `query_all`.
        let handle = unsafe {
            (client.lib.fns.ugc.create_query_user_ugc_request)(
                client.ugc,
                account,
                list.raw(),
                matching.0,
                order.raw(),
                app,
                app,
                page,
            )
        };
        UgcQuery::new(client, handle, "CreateQueryUserUGCRequest")
    }

    /// A query for the details of `items` (`CreateQueryUGCDetailsRequest`).
    ///
    /// # Errors
    ///
    /// [`SteamError::TooMany`] for more ids than a `uint32` counts;
    /// [`SteamError::Refused`] when Steam makes no query.
    pub fn query_details(&self, items: &[ItemId]) -> Result<UgcQuery, SteamError> {
        let count = u32::try_from(items.len()).map_err(|_| SteamError::TooMany {
            what: "items in one details query",
            max: u32::MAX as usize,
        })?;
        // A copy: Steam takes the ids through a mutable pointer.
        let mut ids: Vec<u64> = items.iter().map(|item| item.0).collect();
        let client = &self.steam.client;
        // SAFETY: as in `query_all`; `ids` is `count` writable ids.
        let handle = unsafe {
            (client.lib.fns.ugc.create_query_ugc_details_request)(
                client.ugc,
                ids.as_mut_ptr(),
                count,
            )
        };
        UgcQuery::new(client, handle, "CreateQueryUGCDetailsRequest")
    }

    /// Sends `query` (`SendQueryUGCRequest`); its [`QueryPage`] arrives
    /// through [`Steam::take`], and its items are read with
    /// [`results`](Self::results). A query is sent once.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] for a query already sent, or when Steam did
    /// not start the call.
    pub fn send(&mut self, query: &mut UgcQuery) -> Result<SteamCall<QueryPage>, SteamError> {
        if query.sent {
            return Err(SteamError::Refused("SendQueryUGCRequest"));
        }
        let client = &self.steam.client;
        // SAFETY: as in `query_all`; `query.handle` is a live query handle.
        let handle =
            unsafe { (client.lib.fns.ugc.send_query_ugc_request)(client.ugc, query.handle) };
        let call = self
            .steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("SendQueryUGCRequest"))?;
        query.sent = true;
        Ok(call)
    }

    /// The items on `page`, read through `query`, the query it answers
    /// (`GetQueryUGCResult`, once per item).
    ///
    /// # Errors
    ///
    /// [`SteamError::Result`] for a page Steam answered with a failure;
    /// [`SteamError::OutOfRange`] for a page that answers another query;
    /// [`SteamError::Refused`] for an item Steam will not hand over.
    pub fn results(
        &self,
        query: &UgcQuery,
        page: &QueryPage,
    ) -> Result<Vec<ItemDetails>, SteamError> {
        if page.handle != query.handle {
            return Err(SteamError::OutOfRange("page"));
        }
        if page.result != EResult::OK {
            return Err(SteamError::Result(page.result));
        }
        let client = &self.steam.client;
        let mut items = Vec::with_capacity(usize::try_from(page.returned).unwrap_or(0));
        for index in 0..page.returned {
            // SAFETY: an all-zero `SteamUgcDetails` is a valid value, being
            // integers and bytes.
            let mut raw: structs::SteamUgcDetails = unsafe { core::mem::zeroed() };
            // SAFETY: as in `query_all`; `query.handle` is live, and `raw` is
            // a writable `SteamUGCDetails_t`.
            let read = unsafe {
                (client.lib.fns.ugc.get_query_ugc_result)(
                    client.ugc,
                    query.handle,
                    index,
                    &raw mut raw,
                )
            };
            if !read {
                return Err(SteamError::Refused("GetQueryUGCResult"));
            }
            let (details, lossy) = ItemDetails::decode(&raw);
            self.count_lossy(lossy);
            items.push(details);
        }
        Ok(items)
    }

    /// Subscribes the player to `item` (`SubscribeItem`); Steam downloads and
    /// installs it.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn subscribe(&mut self, item: ItemId) -> Result<SteamCall<Subscribed>, SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `query_all`.
        let handle = unsafe { (client.lib.fns.ugc.subscribe_item)(client.ugc, item.0) };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("SubscribeItem"))
    }

    /// Unsubscribes the player from `item` (`UnsubscribeItem`); Steam removes
    /// it once the game exits.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn unsubscribe(&mut self, item: ItemId) -> Result<SteamCall<Unsubscribed>, SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `query_all`.
        let handle = unsafe { (client.lib.fns.ugc.unsubscribe_item)(client.ugc, item.0) };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("UnsubscribeItem"))
    }

    /// Every item the player subscribes to for the running game, those
    /// disabled on this machine included when `include_disabled`
    /// (`GetNumSubscribedItems`, then `GetSubscribedItems`).
    #[must_use]
    pub fn subscribed_items(&self, include_disabled: bool) -> Vec<ItemId> {
        let client = &self.steam.client;
        // SAFETY: as in `query_all`.
        let count =
            unsafe { (client.lib.fns.ugc.get_num_subscribed_items)(client.ugc, include_disabled) };
        let mut ids = vec![0_u64; usize::try_from(count).unwrap_or(0)];
        let capacity = u32::try_from(ids.len()).unwrap_or(0);
        // SAFETY: as in `query_all`; `ids` is `capacity` writable ids.
        let written = unsafe {
            (client.lib.fns.ugc.get_subscribed_items)(
                client.ugc,
                ids.as_mut_ptr(),
                capacity,
                include_disabled,
            )
        };
        // Never past the buffer, whatever Steam's count says.
        ids.truncate(usize::try_from(written).unwrap_or(0));
        ids.into_iter().map(ItemId).collect()
    }

    /// What the client knows of `item` (`GetItemState`).
    #[must_use]
    pub fn state(&self, item: ItemId) -> ItemState {
        let client = &self.steam.client;
        // SAFETY: as in `query_all`.
        ItemState(unsafe { (client.lib.fns.ugc.get_item_state)(client.ugc, item.0) })
    }

    /// Where `item` is installed, or `None` when it is not
    /// (`GetItemInstallInfo`). The folder is read through the growing buffer
    /// every string read here uses.
    ///
    /// # Errors
    ///
    /// [`SteamError::Truncated`] for a folder past
    /// [`MAX_TEXT_BYTES`](crate::MAX_TEXT_BYTES).
    pub fn install_info(&self, item: ItemId) -> Result<Option<InstallInfo>, SteamError> {
        let client = &self.steam.client;
        let ((installed, size_on_disk, updated), [folder]) =
            grow("GetItemInstallInfo", |[folder], capacity| {
                let capacity = u32::try_from(capacity).ok()?;
                let mut size = 0_u64;
                let mut updated = 0_u32;
                // SAFETY: as in `query_all`; both out-parameters are writable
                // and `folder` is `capacity` writable bytes.
                let installed = unsafe {
                    (client.lib.fns.ugc.get_item_install_info)(
                        client.ugc,
                        item.0,
                        &raw mut size,
                        folder.as_mut_ptr().cast::<c_char>(),
                        capacity,
                        &raw mut updated,
                    )
                };
                Some((installed, size, updated))
            })?;
        if !installed {
            return Ok(None);
        }
        let (folder, lossy) = fixed_string(&folder);
        self.count_lossy(lossy);
        Ok(Some(InstallInfo {
            size_on_disk,
            folder: PathBuf::from(folder),
            updated,
        }))
    }

    /// How far `item`'s download is, or `None` when none is known
    /// (`GetItemDownloadInfo`).
    #[must_use]
    pub fn download_progress(&self, item: ItemId) -> Option<DownloadProgress> {
        let client = &self.steam.client;
        let mut downloaded = 0_u64;
        let mut total = 0_u64;
        // SAFETY: as in `query_all`; both out-parameters are writable.
        let known = unsafe {
            (client.lib.fns.ugc.get_item_download_info)(
                client.ugc,
                item.0,
                &raw mut downloaded,
                &raw mut total,
            )
        };
        known.then_some(DownloadProgress { downloaded, total })
    }

    /// Downloads or updates `item` now — ahead of Steam's own queue when
    /// `high_priority` (`DownloadItem`). The end arrives as
    /// [`SteamEvent::WorkshopItemDownloaded`](crate::SteamEvent::WorkshopItemDownloaded).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam will not start it.
    pub fn download(&self, item: ItemId, high_priority: bool) -> Result<(), SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `query_all`.
        let started =
            unsafe { (client.lib.fns.ugc.download_item)(client.ugc, item.0, high_priority) };
        started
            .then_some(())
            .ok_or(SteamError::Refused("DownloadItem"))
    }

    /// Makes an empty item of `file_type` for the running game
    /// (`CreateItem`); fill it with [`start_update`](Self::start_update). An
    /// item made through a token that is then dropped stays on the Workshop,
    /// empty — the call's effect is kept, its answer abandoned.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn create_item(
        &mut self,
        file_type: FileType,
    ) -> Result<SteamCall<ItemCreated>, SteamError> {
        let client = &self.steam.client;
        let app = self.steam.app.0;
        // SAFETY: as in `query_all`.
        let handle = unsafe { (client.lib.fns.ugc.create_item)(client.ugc, app, file_type.0) };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("CreateItem"))
    }

    /// Opens an update of `item`, one of the player's own
    /// (`StartItemUpdate`); stage changes on it, then [`submit`](Self::submit)
    /// it.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam opens none.
    pub fn start_update(&self, item: ItemId) -> Result<ItemUpdate, SteamError> {
        let client = &self.steam.client;
        let app = self.steam.app.0;
        // SAFETY: as in `query_all`.
        let handle = unsafe { (client.lib.fns.ugc.start_item_update)(client.ugc, app, item.0) };
        if handle == INVALID_HANDLE {
            return Err(SteamError::Refused("StartItemUpdate"));
        }
        Ok(ItemUpdate::new(Arc::clone(client), handle, item))
    }

    /// Uploads what `update` staged, with `change_note` shown in the item's
    /// history (`SubmitItemUpdate`). The update is consumed: nothing can be
    /// staged on it afterwards. Answers the call and a [`Submission`] that
    /// reports the upload's progress.
    ///
    /// # Errors
    ///
    /// [`SteamError::TooLong`] for a note past [`MAX_CHANGE_NOTE_LENGTH`] and
    /// [`SteamError::InteriorNul`], both before the call, which leave the
    /// update unsubmitted and gone; [`SteamError::Refused`] when Steam did not
    /// start the call.
    pub fn submit(
        &mut self,
        update: ItemUpdate,
        change_note: &str,
    ) -> Result<(SteamCall<ItemSubmitted>, Submission), SteamError> {
        let note = c_string(change_note, "change note", MAX_CHANGE_NOTE_LENGTH)?;
        let client = &self.steam.client;
        // SAFETY: as in `query_all`; `update.handle` is the live handle
        // `StartItemUpdate` opened, and `note` is NUL-terminated for the
        // call.
        let handle = unsafe {
            (client.lib.fns.ugc.submit_item_update)(client.ugc, update.handle, note.as_ptr())
        };
        let call = self
            .steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("SubmitItemUpdate"))?;
        Ok((call, update.into_submission()))
    }

    /// Deletes `item`, one of the player's own (`DeleteItem`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn delete_item(&mut self, item: ItemId) -> Result<SteamCall<ItemDeleted>, SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `query_all`.
        let handle = unsafe { (client.lib.fns.ugc.delete_item)(client.ugc, item.0) };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("DeleteItem"))
    }

    /// Counts a string read lossily, as every string read does.
    fn count_lossy(&self, lossy: bool) {
        if lossy {
            let count = &self.steam.lossy_strings;
            count.set(count.get() + 1);
        }
    }
}

/// The answer to [`Workshop::subscribe`]
/// (`RemoteStorageSubscribePublishedFileResult_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subscribed {
    /// `EResult::OK`, or why not.
    pub result: EResult,
    /// The item.
    pub item: ItemId,
}

impl Answer for Subscribed {
    const ROW: CallRow = CallRow {
        base: Base::RemoteStorage,
        offset: 13,
        #[cfg(test)]
        name: "RemoteStorageSubscribePublishedFileResult_t",
        size: size_of::<structs::SubscribeResult>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::SubscribeResult>(bytes)?;
        Some(Self {
            result: EResult(raw.result),
            item: ItemId(raw.item),
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

/// The answer to [`Workshop::unsubscribe`]
/// (`RemoteStorageUnsubscribePublishedFileResult_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsubscribed {
    /// `EResult::OK`, or why not.
    pub result: EResult,
    /// The item.
    pub item: ItemId,
}

impl Answer for Unsubscribed {
    const ROW: CallRow = CallRow {
        base: Base::RemoteStorage,
        offset: 15,
        #[cfg(test)]
        name: "RemoteStorageUnsubscribePublishedFileResult_t",
        size: size_of::<structs::UnsubscribeResult>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::UnsubscribeResult>(bytes)?;
        Some(Self {
            result: EResult(raw.result),
            item: ItemId(raw.item),
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

/// The answer to [`Workshop::create_item`] (`CreateItemResult_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemCreated {
    /// `EResult::OK`, or why not.
    pub result: EResult,
    /// The new item.
    pub item: ItemId,
    /// The player must accept the Workshop legal agreement before the item
    /// can be seen; show it with the overlay's Workshop page.
    pub needs_agreement: bool,
}

impl Answer for ItemCreated {
    const ROW: CallRow = CallRow {
        base: Base::Ugc,
        offset: 3,
        #[cfg(test)]
        name: "CreateItemResult_t",
        size: size_of::<structs::CreateItemResult>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::CreateItemResult>(bytes)?;
        Some(Self {
            result: EResult(raw.result),
            item: ItemId(raw.item),
            needs_agreement: raw.needs_agreement != 0,
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

/// The answer to [`Workshop::submit`] (`SubmitItemUpdateResult_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSubmitted {
    /// `EResult::OK`, or why not.
    pub result: EResult,
    /// The item.
    pub item: ItemId,
    /// As [`ItemCreated::needs_agreement`].
    pub needs_agreement: bool,
}

impl Answer for ItemSubmitted {
    const ROW: CallRow = CallRow {
        base: Base::Ugc,
        offset: 4,
        #[cfg(test)]
        name: "SubmitItemUpdateResult_t",
        size: size_of::<structs::SubmitItemUpdateResult>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::SubmitItemUpdateResult>(bytes)?;
        Some(Self {
            result: EResult(raw.result),
            item: ItemId(raw.item),
            needs_agreement: raw.needs_agreement != 0,
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

/// The answer to [`Workshop::delete_item`] (`DeleteItemResult_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemDeleted {
    /// `EResult::OK`, or why not.
    pub result: EResult,
    /// The item.
    pub item: ItemId,
}

impl Answer for ItemDeleted {
    const ROW: CallRow = CallRow {
        base: Base::Ugc,
        offset: 17,
        #[cfg(test)]
        name: "DeleteItemResult_t",
        size: size_of::<structs::DeleteItemResult>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::DeleteItemResult>(bytes)?;
        Some(Self {
            result: EResult(raw.result),
            item: ItemId(raw.item),
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

#[cfg(test)]
mod tests;
