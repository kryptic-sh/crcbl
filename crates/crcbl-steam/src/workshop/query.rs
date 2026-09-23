//! A Workshop query: the handle Steam hands out, released once; the page it
//! answers; and each item on the page.

use std::{marker::PhantomData, sync::Arc};

use super::{FileType, INVALID_HANDLE, ItemId, UGC_RESULTS_PER_PAGE, Visibility};
use crate::{
    AppId, EResult, Steam, SteamError, SteamId,
    call::{CallRow, private::Answer},
    callbacks::{Base, fixed_string, read},
    client::Client,
    error::c_string,
    ffi::{UgcQueryHandle, structs},
};

/// A query Steam made (`UGCQueryHandle_t`), from
/// [`Workshop::query_all`](super::Workshop::query_all) and its siblings.
/// Narrow it with its filters, send it with
/// [`Workshop::send`](super::Workshop::send), and read its page with
/// [`Workshop::results`](super::Workshop::results).
///
/// Released (`ReleaseQueryUGCRequest`) exactly once, when dropped — sent or
/// not. Dropping it before its page arrives leaves the page with no query to
/// read it through.
#[derive(Debug)]
pub struct UgcQuery {
    client: Arc<Client>,
    pub(super) handle: UgcQueryHandle,
    /// Set by `Workshop::send`: filters then no longer apply, and a second
    /// send is refused.
    pub(super) sent: bool,
    _not_send: PhantomData<*const ()>,
}

impl UgcQuery {
    /// Wraps a handle a `CreateQuery…` call answered, refusing the invalid
    /// one — which is never released, having never been made.
    pub(super) fn new(
        client: &Arc<Client>,
        handle: UgcQueryHandle,
        call: &'static str,
    ) -> Result<Self, SteamError> {
        if handle == INVALID_HANDLE {
            return Err(SteamError::Refused(call));
        }
        Ok(Self {
            client: Arc::clone(client),
            handle,
            sent: false,
            _not_send: PhantomData,
        })
    }

    /// Matches only items carrying `tag` (`AddRequiredTag`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`]; [`SteamError::Refused`] after the query
    /// was sent, or when Steam refuses the filter.
    pub fn require_tag(&mut self, tag: &str) -> Result<(), SteamError> {
        let tag = c_string(tag, "tag", usize::MAX)?;
        self.filter("AddRequiredTag", |client, handle| {
            // SAFETY: see `filter`; `tag` is NUL-terminated for the call.
            unsafe { (client.lib.fns.ugc.add_required_tag)(client.ugc, handle, tag.as_ptr()) }
        })
    }

    /// Matches only items without `tag` (`AddExcludedTag`).
    ///
    /// # Errors
    ///
    /// As [`require_tag`](Self::require_tag).
    pub fn exclude_tag(&mut self, tag: &str) -> Result<(), SteamError> {
        let tag = c_string(tag, "tag", usize::MAX)?;
        self.filter("AddExcludedTag", |client, handle| {
            // SAFETY: see `filter`; `tag` is NUL-terminated for the call.
            unsafe { (client.lib.fns.ugc.add_excluded_tag)(client.ugc, handle, tag.as_ptr()) }
        })
    }

    /// Matches items whose text contains `text` (`SetSearchText`); pair it
    /// with [`QueryOrder::RANKED_BY_TEXT_SEARCH`](super::QueryOrder::RANKED_BY_TEXT_SEARCH).
    ///
    /// # Errors
    ///
    /// As [`require_tag`](Self::require_tag).
    pub fn search_text(&mut self, text: &str) -> Result<(), SteamError> {
        let text = c_string(text, "search text", usize::MAX)?;
        self.filter("SetSearchText", |client, handle| {
            // SAFETY: see `filter`; `text` is NUL-terminated for the call.
            unsafe { (client.lib.fns.ugc.set_search_text)(client.ugc, handle, text.as_ptr()) }
        })
    }

    /// Returns each item's whole description rather than its first 255
    /// bytes (`SetReturnLongDescription`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] after the query was sent, or when Steam
    /// refuses.
    pub fn long_description(&mut self, long: bool) -> Result<(), SteamError> {
        self.filter("SetReturnLongDescription", |client, handle| {
            // SAFETY: see `filter`.
            unsafe { (client.lib.fns.ugc.set_return_long_description)(client.ugc, handle, long) }
        })
    }

    /// Applies one filter call, refused without a call once the query was
    /// sent.
    ///
    /// Every `call` passed here is made with `self.client`'s live `ugc` on
    /// the pump thread — `UgcQuery` is `!Send` — and this query's live
    /// handle.
    fn filter(
        &mut self,
        name: &'static str,
        call: impl FnOnce(&Client, UgcQueryHandle) -> bool,
    ) -> Result<(), SteamError> {
        if self.sent {
            return Err(SteamError::Refused(name));
        }
        call(&self.client, self.handle)
            .then_some(())
            .ok_or(SteamError::Refused(name))
    }
}

impl Drop for UgcQuery {
    fn drop(&mut self) {
        let client = &self.client;
        // SAFETY: `client.ugc` is live; `UgcQuery` is `!Send`, so this is the
        // pump thread; the handle was made for this value and is released
        // here once.
        let released =
            unsafe { (client.lib.fns.ugc.release_query_ugc_request)(client.ugc, self.handle) };
        if !released {
            // Nothing is left to release either way, and a `Drop` has no one
            // to answer; the log is the record.
            log::warn!(
                "steam: ReleaseQueryUGCRequest refused query handle {}",
                self.handle
            );
        }
    }
}

/// The answer to [`Workshop::send`](super::Workshop::send)
/// (`SteamUGCQueryCompleted_t`): one page of a query's results, read with
/// [`Workshop::results`](super::Workshop::results).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryPage {
    /// The query it answers.
    pub(super) handle: UgcQueryHandle,
    /// `EResult::OK`, or why there is no page.
    pub result: EResult,
    /// How many items this page holds.
    pub returned: u32,
    /// How many items match in all, over every page.
    pub total: u32,
    /// Whether Steam answered from its on-disk cache.
    pub cached: bool,
}

impl QueryPage {
    /// The page after `page` — the page number this one was asked for —
    /// or `None` when this page reaches the last match. Pages count from 1
    /// and hold [`UGC_RESULTS_PER_PAGE`] items each.
    #[must_use]
    pub fn next_page(&self, page: u32) -> Option<u32> {
        let seen = u64::from(page) * u64::from(UGC_RESULTS_PER_PAGE);
        (seen < u64::from(self.total)).then(|| page.checked_add(1))?
    }
}

impl Answer for QueryPage {
    const ROW: CallRow = CallRow {
        base: Base::Ugc,
        offset: 1,
        #[cfg(test)]
        name: "SteamUGCQueryCompleted_t",
        size: size_of::<structs::SteamUgcQueryCompleted>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::SteamUgcQueryCompleted>(bytes)?;
        Some(Self {
            handle: raw.handle,
            result: EResult(raw.result),
            returned: raw.returned,
            total: raw.total,
            cached: raw.cached != 0,
        })
    }

    // The query value owns the handle, and releases it itself.
    fn abandon(_: &[u8], _: &Client) {}
}

/// One Workshop item, as a query page reports it (`SteamUGCDetails_t`).
/// The legacy single-file handles and sizes are not carried; the item's
/// files are wherever [`Workshop::install_info`](super::Workshop::install_info)
/// says.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemDetails {
    /// The item.
    pub item: ItemId,
    /// `EResult::OK`, or why its details are missing.
    pub result: EResult,
    /// Its kind.
    pub file_type: FileType,
    /// The app that made it.
    pub creator_app: AppId,
    /// The app that uses it.
    pub consumer_app: AppId,
    /// Its title.
    pub title: String,
    /// Its description — the first 255 bytes unless the query asked for
    /// [`UgcQuery::long_description`].
    pub description: String,
    /// Who made it.
    pub owner: SteamId,
    /// When it was made, in Unix seconds.
    pub created: u32,
    /// When it was last updated, in Unix seconds.
    pub updated: u32,
    /// When the queried user added it to the queried list, in Unix seconds;
    /// zero where that means nothing.
    pub added_to_user_list: u32,
    /// Who can see it.
    pub visibility: Visibility,
    /// Whether it is banned.
    pub banned: bool,
    /// Whether the developer accepted it for use.
    pub accepted: bool,
    /// Its tags.
    pub tags: Vec<String>,
    /// Whether Steam cut the tag list short.
    pub tags_truncated: bool,
    /// Its URL, for a video or a website item.
    pub url: String,
    /// Votes up.
    pub votes_up: u32,
    /// Votes down.
    pub votes_down: u32,
    /// Steam's score for it, from 0 to 1.
    pub score: f32,
    /// How many items it holds, for a collection.
    pub children: u32,
    /// The size of all its files, the preview's excluded, in bytes.
    pub total_size: u64,
}

impl ItemDetails {
    /// Copies an item out of Steam's struct, and whether any of its text had
    /// to be read lossily.
    pub(super) fn decode(raw: &structs::SteamUgcDetails) -> (Self, bool) {
        // Copies, never references: the struct is packed.
        let (title, title_lossy) = fixed_string(&{ raw.title });
        let (description, description_lossy) = fixed_string(&{ raw.description });
        let (tags, tags_lossy) = fixed_string(&{ raw.tags });
        let (url, url_lossy) = fixed_string(&{ raw.url });
        let details = Self {
            item: ItemId(raw.item),
            result: EResult(raw.result),
            file_type: FileType(raw.file_type),
            creator_app: AppId(raw.creator_app),
            consumer_app: AppId(raw.consumer_app),
            title,
            description,
            owner: SteamId(raw.owner),
            created: raw.created,
            updated: raw.updated,
            added_to_user_list: raw.added_to_user_list,
            visibility: Visibility::from_raw(raw.visibility),
            banned: raw.banned != 0,
            accepted: raw.accepted != 0,
            tags: tags
                .split(',')
                .filter(|tag| !tag.is_empty())
                .map(str::to_owned)
                .collect(),
            tags_truncated: raw.tags_truncated != 0,
            url,
            votes_up: raw.votes_up,
            votes_down: raw.votes_down,
            score: f32::from_bits(raw.score),
            children: raw.children,
            total_size: raw.total_files_size,
        };
        let lossy = title_lossy || description_lossy || tags_lossy || url_lossy;
        (details, lossy)
    }
}
