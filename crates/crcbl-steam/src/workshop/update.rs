//! An update to one of the player's items: changes staged on Steam's update
//! handle, then submitted — after which the handle only reports progress.

use core::ffi::c_char;
use std::{ffi::CString, marker::PhantomData, path::Path, sync::Arc};

use super::{
    ItemId, MAX_ITEM_DESCRIPTION_LENGTH, MAX_ITEM_METADATA_LENGTH, MAX_ITEM_TAG_LENGTH,
    MAX_ITEM_TITLE_LENGTH, Visibility,
};
use crate::{
    SteamError,
    client::Client,
    error::c_string,
    ffi::{UgcUpdateHandle, structs::SteamParamStringArray},
};

/// Changes to one item, staged (`UGCUpdateHandle_t`), from
/// [`Workshop::start_update`](super::Workshop::start_update). Each setter
/// stages one change; [`Workshop::submit`](super::Workshop::submit) consumes
/// the update and uploads them all, so nothing can be staged after it.
///
/// Steam has no call to abandon an update: one dropped unsubmitted is simply
/// never uploaded.
#[derive(Debug)]
pub struct ItemUpdate {
    client: Arc<Client>,
    pub(super) handle: UgcUpdateHandle,
    item: ItemId,
    _not_send: PhantomData<*const ()>,
}

impl ItemUpdate {
    pub(super) const fn new(client: Arc<Client>, handle: UgcUpdateHandle, item: ItemId) -> Self {
        Self {
            client,
            handle,
            item,
            _not_send: PhantomData,
        }
    }

    /// The item being updated.
    #[must_use]
    pub const fn item(&self) -> ItemId {
        self.item
    }

    /// Stages a new title (`SetItemTitle`).
    ///
    /// # Errors
    ///
    /// [`SteamError::TooLong`] past [`MAX_ITEM_TITLE_LENGTH`] and
    /// [`SteamError::InteriorNul`], before the call; [`SteamError::Refused`]
    /// when Steam refuses it.
    pub fn set_title(&mut self, title: &str) -> Result<(), SteamError> {
        let title = c_string(title, "title", MAX_ITEM_TITLE_LENGTH)?;
        self.stage("SetItemTitle", |client, handle| {
            // SAFETY: see `stage`; `title` is NUL-terminated for the call.
            unsafe { (client.lib.fns.ugc.set_item_title)(client.ugc, handle, title.as_ptr()) }
        })
    }

    /// Stages a new description (`SetItemDescription`).
    ///
    /// # Errors
    ///
    /// As [`set_title`](Self::set_title), past
    /// [`MAX_ITEM_DESCRIPTION_LENGTH`].
    pub fn set_description(&mut self, description: &str) -> Result<(), SteamError> {
        let description = c_string(description, "description", MAX_ITEM_DESCRIPTION_LENGTH)?;
        self.stage("SetItemDescription", |client, handle| {
            // SAFETY: see `stage`; `description` is NUL-terminated for the
            // call.
            unsafe {
                (client.lib.fns.ugc.set_item_description)(client.ugc, handle, description.as_ptr())
            }
        })
    }

    /// Stages the game's own metadata for the item, which players never see
    /// (`SetItemMetadata`).
    ///
    /// # Errors
    ///
    /// As [`set_title`](Self::set_title), past [`MAX_ITEM_METADATA_LENGTH`].
    pub fn set_metadata(&mut self, metadata: &str) -> Result<(), SteamError> {
        let metadata = c_string(metadata, "metadata", MAX_ITEM_METADATA_LENGTH)?;
        self.stage("SetItemMetadata", |client, handle| {
            // SAFETY: see `stage`; `metadata` is NUL-terminated for the call.
            unsafe { (client.lib.fns.ugc.set_item_metadata)(client.ugc, handle, metadata.as_ptr()) }
        })
    }

    /// Stages who can see the item (`SetItemVisibility`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam refuses it.
    pub fn set_visibility(&mut self, visibility: Visibility) -> Result<(), SteamError> {
        self.stage("SetItemVisibility", |client, handle| {
            // SAFETY: see `stage`.
            unsafe {
                (client.lib.fns.ugc.set_item_visibility)(client.ugc, handle, visibility.raw())
            }
        })
    }

    /// Stages the item's tags, replacing the ones it had (`SetItemTags`).
    ///
    /// # Errors
    ///
    /// [`SteamError::TooLong`] for a tag past [`MAX_ITEM_TAG_LENGTH`],
    /// [`SteamError::OutOfRange`] for one containing a comma (Steam joins
    /// tags with commas) and [`SteamError::InteriorNul`], all before the call;
    /// [`SteamError::Refused`] when Steam refuses them.
    pub fn set_tags(&mut self, tags: &[&str]) -> Result<(), SteamError> {
        let tags = tags
            .iter()
            .map(|tag| {
                if tag.contains(',') {
                    return Err(SteamError::OutOfRange("tag"));
                }
                c_string(tag, "tag", MAX_ITEM_TAG_LENGTH)
            })
            .collect::<Result<Vec<CString>, _>>()?;
        let pointers: Vec<*const c_char> = tags.iter().map(|tag| tag.as_ptr()).collect();
        let list = SteamParamStringArray {
            strings: pointers.as_ptr(),
            count: i32::try_from(pointers.len()).map_err(|_| SteamError::TooMany {
                what: "tags",
                max: i32::MAX as usize,
            })?,
        };
        self.stage("SetItemTags", |client, handle| {
            // SAFETY: see `stage`; `list` points at `pointers`, each a
            // NUL-terminated string in `tags`, all alive for the call. No
            // admin tags.
            unsafe {
                (client.lib.fns.ugc.set_item_tags)(client.ugc, handle, &raw const list, false)
            }
        })
    }

    /// Stages the folder whose files become the item's content
    /// (`SetItemContent`).
    ///
    /// # Errors
    ///
    /// [`SteamError::OutOfRange`] for a folder that is not absolute or not
    /// UTF-8 — Steam takes an absolute path, as UTF-8 — before the call;
    /// [`SteamError::Refused`] when Steam refuses it.
    pub fn set_content(&mut self, folder: &Path) -> Result<(), SteamError> {
        let folder = absolute(folder, "content folder")?;
        self.stage("SetItemContent", |client, handle| {
            // SAFETY: see `stage`; `folder` is NUL-terminated for the call.
            unsafe { (client.lib.fns.ugc.set_item_content)(client.ugc, handle, folder.as_ptr()) }
        })
    }

    /// Stages the item's preview image (`SetItemPreview`).
    ///
    /// # Errors
    ///
    /// As [`set_content`](Self::set_content), for the file.
    pub fn set_preview(&mut self, file: &Path) -> Result<(), SteamError> {
        let file = absolute(file, "preview file")?;
        self.stage("SetItemPreview", |client, handle| {
            // SAFETY: see `stage`; `file` is NUL-terminated for the call.
            unsafe { (client.lib.fns.ugc.set_item_preview)(client.ugc, handle, file.as_ptr()) }
        })
    }

    /// Makes one staging call, `Refused` naming it on Steam's `false`.
    ///
    /// Every `call` passed here is made with `self.client`'s live `ugc` on
    /// the pump thread — `ItemUpdate` is `!Send` — and this update's live
    /// handle.
    fn stage(
        &mut self,
        name: &'static str,
        call: impl FnOnce(&Client, UgcUpdateHandle) -> bool,
    ) -> Result<(), SteamError> {
        call(&self.client, self.handle)
            .then_some(())
            .ok_or(SteamError::Refused(name))
    }

    /// The submitted update, which only reports progress.
    pub(super) fn into_submission(self) -> Submission {
        Submission {
            client: self.client,
            handle: self.handle,
            item: self.item,
            _not_send: PhantomData,
        }
    }
}

/// A path as the NUL-terminated UTF-8 Steam takes, refused unless absolute.
fn absolute(path: &Path, argument: &'static str) -> Result<CString, SteamError> {
    if !path.is_absolute() {
        return Err(SteamError::OutOfRange(argument));
    }
    let text = path.to_str().ok_or(SteamError::OutOfRange(argument))?;
    c_string(text, argument, usize::MAX)
}

/// A submitted update, from [`Workshop::submit`](super::Workshop::submit):
/// the upload's progress, while it runs. Its end arrives as the call's
/// [`ItemSubmitted`](super::ItemSubmitted).
#[derive(Debug)]
pub struct Submission {
    client: Arc<Client>,
    handle: UgcUpdateHandle,
    item: ItemId,
    _not_send: PhantomData<*const ()>,
}

impl Submission {
    /// The item being uploaded.
    #[must_use]
    pub const fn item(&self) -> ItemId {
        self.item
    }

    /// How far the upload is (`GetItemUpdateProgress`).
    #[must_use]
    pub fn progress(&self) -> UpdateProgress {
        let client = &self.client;
        let mut processed = 0_u64;
        let mut total = 0_u64;
        // SAFETY: `client.ugc` is live; `Submission` is `!Send`, so this is
        // the pump thread; the handle is the submitted update's, and both
        // out-parameters are writable.
        let status = unsafe {
            (client.lib.fns.ugc.get_item_update_progress)(
                client.ugc,
                self.handle,
                &raw mut processed,
                &raw mut total,
            )
        };
        UpdateProgress {
            status: UpdateStatus::from_raw(status),
            processed,
            total,
        }
    }
}

/// How far an upload is (`GetItemUpdateProgress`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateProgress {
    /// What it is doing.
    pub status: UpdateStatus,
    /// Bytes processed so far in this stage.
    pub processed: u64,
    /// Bytes in this stage.
    pub total: u64,
}

/// What an upload is doing (`EItemUpdateStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateStatus {
    /// Steam knows no running upload for the handle — it may have finished;
    /// the call's answer says (`k_EItemUpdateStatusInvalid`).
    Invalid,
    /// `k_EItemUpdateStatusPreparingConfig`.
    PreparingConfig,
    /// Reading the content files (`k_EItemUpdateStatusPreparingContent`).
    PreparingContent,
    /// `k_EItemUpdateStatusUploadingContent`.
    UploadingContent,
    /// `k_EItemUpdateStatusUploadingPreviewFile`.
    UploadingPreview,
    /// `k_EItemUpdateStatusCommittingChanges`.
    CommittingChanges,
    /// A value this crate does not name.
    Other(i32),
}

impl UpdateStatus {
    const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Invalid,
            1 => Self::PreparingConfig,
            2 => Self::PreparingContent,
            3 => Self::UploadingContent,
            4 => Self::UploadingPreview,
            5 => Self::CommittingChanges,
            other => Self::Other(other),
        }
    }
}
