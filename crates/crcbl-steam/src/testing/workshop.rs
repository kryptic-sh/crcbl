//! The fake `ISteamUGC`: scripted handles and answers, and every call
//! recorded as one line — its name, then its arguments, `|`-separated — in
//! order, for the tests to read back.

use std::ffi::{CStr, c_char, c_void};

use super::{accessor, script};
use crate::ffi::{
    ISteamUgc, PublishedFileId, SteamApiCall, UgcQueryHandle, UgcUpdateHandle,
    manifest::UgcFns,
    structs::{SteamParamStringArray, SteamUgcDetails},
};

/// What the fake Workshop answers, and what it has seen.
#[derive(Debug, Default)]
pub(crate) struct FakeWorkshop {
    /// Every call, as one line each.
    pub(crate) log: Vec<String>,
    /// What every `CreateQuery…` call answers; all ones is invalid.
    pub(crate) query: UgcQueryHandle,
    /// What every call-returning function answers; `0` is invalid.
    pub(crate) call: SteamApiCall,
    /// What `GetQueryUGCResult` copies out, by index; an index past the end
    /// answers `false`.
    pub(crate) details: Vec<SteamUgcDetails>,
    /// What `StartItemUpdate` answers; all ones is invalid.
    pub(crate) update: UgcUpdateHandle,
    /// What `GetItemUpdateProgress` answers: status, processed, total.
    pub(crate) progress: (i32, u64, u64),
    /// The items `GetSubscribedItems` copies out; `GetNumSubscribedItems`
    /// answers `count`, or their count when it is unset.
    pub(crate) subscribed: Vec<u64>,
    pub(crate) count: Option<u32>,
    /// What `GetItemState` answers.
    pub(crate) state: u32,
    /// What `GetItemInstallInfo` answers: size, folder, timestamp; `None`
    /// answers `false`.
    pub(crate) install: Option<(u64, Vec<u8>, u32)>,
    /// Every folder buffer size `GetItemInstallInfo` was offered.
    pub(crate) install_offered: Vec<u32>,
    /// What `GetItemDownloadInfo` answers; `None` answers `false`.
    pub(crate) download_info: Option<(u64, u64)>,
}

fn text(ptr: *const c_char) -> String {
    // SAFETY: every caller passes a NUL-terminated string.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

fn log(line: String) {
    script(|s| s.workshop.log.push(line));
}

/// Logs a bool-returning call, and answers `false` when the script refuses.
fn answer(line: String) -> bool {
    log(line);
    script(|s| !s.refuse)
}

fn call(line: String) -> SteamApiCall {
    log(line);
    script(|s| s.workshop.call)
}

fn query(line: String) -> UgcQueryHandle {
    log(line);
    script(|s| s.workshop.query)
}

pub(super) const UGC: UgcFns = UgcFns {
    accessor: ugc_accessor,
    create_query_user_ugc_request: fake_create_query_user_ugc_request,
    create_query_all_ugc_request_page: fake_create_query_all_ugc_request_page,
    create_query_ugc_details_request: fake_create_query_ugc_details_request,
    send_query_ugc_request: fake_send_query_ugc_request,
    get_query_ugc_result: fake_get_query_ugc_result,
    release_query_ugc_request: fake_release_query_ugc_request,
    add_required_tag: fake_add_required_tag,
    add_excluded_tag: fake_add_excluded_tag,
    set_search_text: fake_set_search_text,
    set_return_long_description: fake_set_return_long_description,
    create_item: fake_create_item,
    start_item_update: fake_start_item_update,
    set_item_title: fake_set_item_title,
    set_item_description: fake_set_item_description,
    set_item_metadata: fake_set_item_metadata,
    set_item_visibility: fake_set_item_visibility,
    set_item_tags: fake_set_item_tags,
    set_item_content: fake_set_item_content,
    set_item_preview: fake_set_item_preview,
    submit_item_update: fake_submit_item_update,
    get_item_update_progress: fake_get_item_update_progress,
    subscribe_item: fake_subscribe_item,
    unsubscribe_item: fake_unsubscribe_item,
    get_num_subscribed_items: fake_get_num_subscribed_items,
    get_subscribed_items: fake_get_subscribed_items,
    get_item_state: fake_get_item_state,
    get_item_install_info: fake_get_item_install_info,
    get_item_download_info: fake_get_item_download_info,
    download_item: fake_download_item,
    delete_item: fake_delete_item,
};

unsafe extern "C" fn ugc_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::UGC.accessor)
}

unsafe extern "C" fn fake_create_query_user_ugc_request(
    _: *mut ISteamUgc,
    account: u32,
    list: i32,
    matching: i32,
    order: i32,
    creator: u32,
    consumer: u32,
    page: u32,
) -> UgcQueryHandle {
    query(format!(
        "CreateQueryUserUGCRequest|{account}|{list}|{matching}|{order}|{creator}|{consumer}|{page}"
    ))
}

unsafe extern "C" fn fake_create_query_all_ugc_request_page(
    _: *mut ISteamUgc,
    order: i32,
    matching: i32,
    creator: u32,
    consumer: u32,
    page: u32,
) -> UgcQueryHandle {
    query(format!(
        "CreateQueryAllUGCRequestPage|{order}|{matching}|{creator}|{consumer}|{page}"
    ))
}

unsafe extern "C" fn fake_create_query_ugc_details_request(
    _: *mut ISteamUgc,
    ids: *mut PublishedFileId,
    count: u32,
) -> UgcQueryHandle {
    let count = usize::try_from(count).unwrap();
    // SAFETY: the caller passes `count` readable ids.
    let ids = unsafe { core::slice::from_raw_parts(ids, count) };
    let ids: Vec<String> = ids.iter().map(u64::to_string).collect();
    query(format!("CreateQueryUGCDetailsRequest|{}", ids.join(",")))
}

unsafe extern "C" fn fake_send_query_ugc_request(
    _: *mut ISteamUgc,
    handle: UgcQueryHandle,
) -> SteamApiCall {
    call(format!("SendQueryUGCRequest|{handle}"))
}

unsafe extern "C" fn fake_get_query_ugc_result(
    _: *mut ISteamUgc,
    handle: UgcQueryHandle,
    index: u32,
    out: *mut SteamUgcDetails,
) -> bool {
    log(format!("GetQueryUGCResult|{handle}|{index}"));
    let details = script(|s| {
        s.workshop
            .details
            .get(usize::try_from(index).unwrap())
            .copied()
    });
    let Some(details) = details else {
        return false;
    };
    // SAFETY: the caller passes a writable `SteamUGCDetails_t`.
    unsafe { out.write(details) };
    true
}

unsafe extern "C" fn fake_release_query_ugc_request(
    _: *mut ISteamUgc,
    handle: UgcQueryHandle,
) -> bool {
    answer(format!("ReleaseQueryUGCRequest|{handle}"))
}

unsafe extern "C" fn fake_add_required_tag(
    _: *mut ISteamUgc,
    handle: UgcQueryHandle,
    tag: *const c_char,
) -> bool {
    answer(format!("AddRequiredTag|{handle}|{}", text(tag)))
}

unsafe extern "C" fn fake_add_excluded_tag(
    _: *mut ISteamUgc,
    handle: UgcQueryHandle,
    tag: *const c_char,
) -> bool {
    answer(format!("AddExcludedTag|{handle}|{}", text(tag)))
}

unsafe extern "C" fn fake_set_search_text(
    _: *mut ISteamUgc,
    handle: UgcQueryHandle,
    search: *const c_char,
) -> bool {
    answer(format!("SetSearchText|{handle}|{}", text(search)))
}

unsafe extern "C" fn fake_set_return_long_description(
    _: *mut ISteamUgc,
    handle: UgcQueryHandle,
    long: bool,
) -> bool {
    answer(format!("SetReturnLongDescription|{handle}|{long}"))
}

unsafe extern "C" fn fake_create_item(_: *mut ISteamUgc, app: u32, kind: i32) -> SteamApiCall {
    call(format!("CreateItem|{app}|{kind}"))
}

unsafe extern "C" fn fake_start_item_update(
    _: *mut ISteamUgc,
    app: u32,
    item: PublishedFileId,
) -> UgcUpdateHandle {
    log(format!("StartItemUpdate|{app}|{item}"));
    script(|s| s.workshop.update)
}

unsafe extern "C" fn fake_set_item_title(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    title: *const c_char,
) -> bool {
    answer(format!("SetItemTitle|{handle}|{}", text(title)))
}

unsafe extern "C" fn fake_set_item_description(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    description: *const c_char,
) -> bool {
    answer(format!("SetItemDescription|{handle}|{}", text(description)))
}

unsafe extern "C" fn fake_set_item_metadata(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    metadata: *const c_char,
) -> bool {
    answer(format!("SetItemMetadata|{handle}|{}", text(metadata)))
}

unsafe extern "C" fn fake_set_item_visibility(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    visibility: i32,
) -> bool {
    answer(format!("SetItemVisibility|{handle}|{visibility}"))
}

unsafe extern "C" fn fake_set_item_tags(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    list: *const SteamParamStringArray,
    admin: bool,
) -> bool {
    // SAFETY: the caller passes a readable list of `count` readable
    // pointers, each to a NUL-terminated string.
    let tags: Vec<String> = unsafe {
        let list = list.read_unaligned();
        let count = usize::try_from(list.count).unwrap();
        core::slice::from_raw_parts(list.strings, count)
            .iter()
            .map(|&tag| text(tag))
            .collect()
    };
    answer(format!("SetItemTags|{handle}|{}|{admin}", tags.join(",")))
}

unsafe extern "C" fn fake_set_item_content(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    folder: *const c_char,
) -> bool {
    answer(format!("SetItemContent|{handle}|{}", text(folder)))
}

unsafe extern "C" fn fake_set_item_preview(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    file: *const c_char,
) -> bool {
    answer(format!("SetItemPreview|{handle}|{}", text(file)))
}

unsafe extern "C" fn fake_submit_item_update(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    note: *const c_char,
) -> SteamApiCall {
    call(format!("SubmitItemUpdate|{handle}|{}", text(note)))
}

unsafe extern "C" fn fake_get_item_update_progress(
    _: *mut ISteamUgc,
    handle: UgcUpdateHandle,
    processed: *mut u64,
    total: *mut u64,
) -> i32 {
    log(format!("GetItemUpdateProgress|{handle}"));
    let (status, done, all) = script(|s| s.workshop.progress);
    // SAFETY: the caller passes two writable counts.
    unsafe {
        processed.write(done);
        total.write(all);
    }
    status
}

unsafe extern "C" fn fake_subscribe_item(_: *mut ISteamUgc, item: PublishedFileId) -> SteamApiCall {
    call(format!("SubscribeItem|{item}"))
}

unsafe extern "C" fn fake_unsubscribe_item(
    _: *mut ISteamUgc,
    item: PublishedFileId,
) -> SteamApiCall {
    call(format!("UnsubscribeItem|{item}"))
}

unsafe extern "C" fn fake_get_num_subscribed_items(_: *mut ISteamUgc, disabled: bool) -> u32 {
    log(format!("GetNumSubscribedItems|{disabled}"));
    script(|s| {
        s.workshop
            .count
            .unwrap_or_else(|| u32::try_from(s.workshop.subscribed.len()).unwrap())
    })
}

unsafe extern "C" fn fake_get_subscribed_items(
    _: *mut ISteamUgc,
    out: *mut PublishedFileId,
    capacity: u32,
    disabled: bool,
) -> u32 {
    log(format!("GetSubscribedItems|{capacity}|{disabled}"));
    let items = script(|s| s.workshop.subscribed.clone());
    let n = items.len().min(usize::try_from(capacity).unwrap());
    // SAFETY: the caller passes `capacity` writable ids, at least `n`.
    unsafe { core::ptr::copy_nonoverlapping(items.as_ptr(), out, n) };
    // Steam's count of every item, which may exceed what fit.
    u32::try_from(items.len()).unwrap()
}

unsafe extern "C" fn fake_get_item_state(_: *mut ISteamUgc, item: PublishedFileId) -> u32 {
    log(format!("GetItemState|{item}"));
    script(|s| s.workshop.state)
}

unsafe extern "C" fn fake_get_item_install_info(
    _: *mut ISteamUgc,
    item: PublishedFileId,
    size: *mut u64,
    folder: *mut c_char,
    capacity: u32,
    updated: *mut u32,
) -> bool {
    let install = script(|s| {
        s.workshop.install_offered.push(capacity);
        s.workshop.install.clone()
    });
    log(format!("GetItemInstallInfo|{item}"));
    let Some((bytes, path, stamp)) = install else {
        return false;
    };
    let room = usize::try_from(capacity).unwrap().saturating_sub(1);
    let n = path.len().min(room);
    // SAFETY: the caller passes writable out-parameters and `capacity`
    // writable folder bytes; `n + 1` is at most that. Steam's copy stops a
    // byte short to leave the NUL, as here.
    unsafe {
        size.write(bytes);
        updated.write(stamp);
        if capacity > 0 {
            core::ptr::copy_nonoverlapping(path.as_ptr(), folder.cast::<u8>(), n);
            folder.add(n).cast::<u8>().write(0);
        }
    }
    true
}

unsafe extern "C" fn fake_get_item_download_info(
    _: *mut ISteamUgc,
    item: PublishedFileId,
    downloaded: *mut u64,
    total: *mut u64,
) -> bool {
    log(format!("GetItemDownloadInfo|{item}"));
    let Some((done, all)) = script(|s| s.workshop.download_info) else {
        return false;
    };
    // SAFETY: the caller passes two writable counts.
    unsafe {
        downloaded.write(done);
        total.write(all);
    }
    true
}

unsafe extern "C" fn fake_download_item(
    _: *mut ISteamUgc,
    item: PublishedFileId,
    high_priority: bool,
) -> bool {
    answer(format!("DownloadItem|{item}|{high_priority}"))
}

unsafe extern "C" fn fake_delete_item(_: *mut ISteamUgc, item: PublishedFileId) -> SteamApiCall {
    call(format!("DeleteItem|{item}"))
}
