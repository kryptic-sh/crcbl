//! The Workshop over the fake: query handles, pages, updates, installs.

use std::path::Path;

use super::*;
use crate::{
    AppId, CallResult, CallState, SteamEvent,
    client::init_on,
    ffi::structs::{self, SteamUgcDetails},
    testing::{self, FakeMsg, FakeWorkshop, completion, payload},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn fake<R>(f: impl FnOnce(&mut FakeWorkshop) -> R) -> R {
    testing::script(|s| f(&mut s.workshop))
}

/// Every logged call whose name is `name`.
fn logged(name: &str) -> Vec<String> {
    let prefix = format!("{name}|");
    fake(|f| {
        f.log
            .iter()
            .filter(|line| line.starts_with(&prefix))
            .cloned()
            .collect()
    })
}

/// A `SteamUGCQueryCompleted_t` answer.
fn query_completed(handle: u64, result: i32, returned: u32, total: u32) -> Vec<u8> {
    use core::mem::offset_of;
    use structs::SteamUgcQueryCompleted as Q;
    payload::<Q>(&[
        (offset_of!(Q, handle), &handle.to_le_bytes()),
        (offset_of!(Q, result), &result.to_le_bytes()),
        (offset_of!(Q, returned), &returned.to_le_bytes()),
        (offset_of!(Q, total), &total.to_le_bytes()),
        (offset_of!(Q, cached), &[1]),
    ])
}

/// One item's details, with a title and a comma-separated tag list.
fn details(item: u64, title: &[u8], tags: &[u8]) -> SteamUgcDetails {
    // SAFETY: an all-zero `SteamUgcDetails` is a valid value, being integers
    // and bytes.
    let mut raw: SteamUgcDetails = unsafe { core::mem::zeroed() };
    raw.item = item;
    raw.result = 1;
    raw.file_type = 15;
    raw.creator_app = 480;
    raw.consumer_app = 480;
    let mut text = [0_u8; 129];
    text[..title.len()].copy_from_slice(title);
    raw.title = text;
    let mut list = [0_u8; 1025];
    list[..tags.len()].copy_from_slice(tags);
    raw.tags = list;
    raw.owner = testing::STEAM_ID;
    raw.visibility = 2;
    raw.accepted = 1;
    raw.votes_up = 7;
    raw.score = 0.5_f32.to_bits();
    raw.total_files_size = 4096;
    raw
}

/// Sends a query for page 1 of every item, as handle 7 answered by call 77.
fn sent(steam: &mut Steam) -> (UgcQuery, SteamCall<QueryPage>) {
    fake(|f| {
        f.query = 7;
        f.call = 77;
    });
    let mut query = steam
        .workshop()
        .query_all(QueryOrder::RANKED_BY_VOTE, MatchingType::ITEMS, 1)
        .unwrap();
    let call = steam.workshop().send(&mut query).unwrap();
    (query, call)
}

/// Pumps a completion of call 77 with `bytes`, answered with `T`'s row.
fn answer<T: CallResult>(steam: &mut Steam, bytes: Vec<u8>) {
    let row = <T as crate::call::private::Answer>::ROW;
    testing::script(|s| {
        // One answer per call at a time: the fake answers the first it holds.
        s.results.retain(|(call, ..)| *call != 77);
        s.results.push((77, bytes, false));
        s.queue.push_back(completion(77, row.id(), row.size));
    });
    steam.pump();
}

fn ready<T: CallResult + core::fmt::Debug>(steam: &mut Steam, call: SteamCall<T>) -> T {
    match steam.take(call) {
        CallState::Ready(answer) => answer,
        other => panic!("not answered: {other:?}"),
    }
}

/// **Query-handle RAII**: released once when dropped, whether it was sent or
/// not, and never before.
#[test]
fn a_query_is_released_once_when_dropped_sent_or_not() {
    let mut steam = steam();
    fake(|f| f.query = 5);
    let unsent = steam
        .workshop()
        .query_all(QueryOrder::BY_PUBLICATION_DATE, MatchingType::ITEMS, 1)
        .unwrap();
    assert!(logged("ReleaseQueryUGCRequest").is_empty());
    drop(unsent);
    assert_eq!(
        logged("ReleaseQueryUGCRequest"),
        ["ReleaseQueryUGCRequest|5"]
    );

    let (query, call) = sent(&mut steam);
    answer::<QueryPage>(&mut steam, query_completed(7, 1, 0, 0));
    let page = ready(&mut steam, call);
    assert!(steam.workshop().results(&query, &page).unwrap().is_empty());
    assert_eq!(logged("ReleaseQueryUGCRequest").len(), 1, "not yet");
    drop(query);
    assert_eq!(
        logged("ReleaseQueryUGCRequest"),
        ["ReleaseQueryUGCRequest|5", "ReleaseQueryUGCRequest|7"]
    );
}

#[test]
fn an_invalid_query_handle_is_refused_and_never_released() {
    let steam_ = &mut steam();
    fake(|f| f.query = u64::MAX);
    assert_eq!(
        steam_
            .workshop()
            .query_all(QueryOrder::RANKED_BY_VOTE, MatchingType::ITEMS, 1)
            .unwrap_err(),
        SteamError::Refused("CreateQueryAllUGCRequestPage")
    );
    assert_eq!(
        steam_.workshop().query_details(&[ItemId(3)]).unwrap_err(),
        SteamError::Refused("CreateQueryUGCDetailsRequest")
    );
    assert!(logged("ReleaseQueryUGCRequest").is_empty());
}

#[test]
fn each_query_passes_the_running_app_its_arguments_and_page() {
    let mut steam = steam();
    fake(|f| f.query = 1);
    let workshop = steam.workshop();
    drop(
        workshop
            .query_all(
                QueryOrder::RANKED_BY_TEXT_SEARCH,
                MatchingType::USABLE_IN_GAME,
                3,
            )
            .unwrap(),
    );
    // The account id is the Steam id's low 32 bits.
    drop(
        workshop
            .query_user(
                SteamId(0x0110_0001_0000_0042),
                UserList::Subscribed,
                MatchingType::ALL,
                UserListOrder::LastUpdatedDesc,
                2,
            )
            .unwrap(),
    );
    drop(workshop.query_details(&[ItemId(9), ItemId(10)]).unwrap());
    assert_eq!(
        logged("CreateQueryAllUGCRequestPage"),
        ["CreateQueryAllUGCRequestPage|11|10|480|480|3"]
    );
    assert_eq!(
        logged("CreateQueryUserUGCRequest"),
        ["CreateQueryUserUGCRequest|66|6|-1|3|480|480|2"]
    );
    assert_eq!(
        logged("CreateQueryUGCDetailsRequest"),
        ["CreateQueryUGCDetailsRequest|9,10"]
    );
}

/// Pages count from 1: page 0 is refused before Steam is asked.
#[test]
fn page_zero_is_refused_before_the_call() {
    let mut steam = steam();
    let workshop = steam.workshop();
    assert_eq!(
        workshop
            .query_all(QueryOrder::RANKED_BY_VOTE, MatchingType::ITEMS, 0)
            .unwrap_err(),
        SteamError::OutOfRange("page")
    );
    assert_eq!(
        workshop
            .query_user(
                SteamId(1),
                UserList::Published,
                MatchingType::ITEMS,
                UserListOrder::TitleAsc,
                0
            )
            .unwrap_err(),
        SteamError::OutOfRange("page")
    );
    assert!(fake(|f| f.log.is_empty()), "{:?}", fake(|f| f.log.clone()));
}

#[test]
fn filters_reach_steam_on_the_query_and_a_refusal_is_named() {
    let mut steam = steam();
    fake(|f| f.query = 4);
    let mut query = steam
        .workshop()
        .query_all(QueryOrder::RANKED_BY_VOTE, MatchingType::ITEMS, 1)
        .unwrap();
    query.require_tag("maps").unwrap();
    query.exclude_tag("broken").unwrap();
    query.search_text("castle").unwrap();
    query.long_description(true).unwrap();
    assert_eq!(
        fake(|f| f.log[1..].to_vec()),
        [
            "AddRequiredTag|4|maps",
            "AddExcludedTag|4|broken",
            "SetSearchText|4|castle",
            "SetReturnLongDescription|4|true",
        ]
    );
    assert_eq!(
        query.require_tag("a\0b").unwrap_err(),
        SteamError::InteriorNul("tag")
    );
    testing::script(|s| s.refuse = true);
    assert_eq!(
        query.require_tag("maps").unwrap_err(),
        SteamError::Refused("AddRequiredTag")
    );
}

/// A query is sent once, and a filter after the send is refused without a
/// call — it could no longer change the page.
#[test]
fn a_query_is_sent_once_and_takes_no_filter_after() {
    let mut steam = steam();
    let (mut query, _call) = sent(&mut steam);
    assert_eq!(
        steam.workshop().send(&mut query).unwrap_err(),
        SteamError::Refused("SendQueryUGCRequest")
    );
    assert_eq!(
        query.search_text("late").unwrap_err(),
        SteamError::Refused("SetSearchText")
    );
    assert_eq!(logged("SendQueryUGCRequest"), ["SendQueryUGCRequest|7"]);
    assert!(logged("SetSearchText").is_empty());
}

#[test]
fn a_send_steam_does_not_start_leaves_the_query_unsent() {
    let mut steam = steam();
    fake(|f| f.query = 7);
    let mut query = steam
        .workshop()
        .query_all(QueryOrder::RANKED_BY_VOTE, MatchingType::ITEMS, 1)
        .unwrap();
    assert_eq!(
        steam.workshop().send(&mut query).unwrap_err(),
        SteamError::Refused("SendQueryUGCRequest")
    );
    // Still unsent: it can be narrowed and sent again.
    query.require_tag("maps").unwrap();
    fake(|f| f.call = 77);
    assert!(steam.workshop().send(&mut query).is_ok());
}

#[test]
fn a_page_decodes_and_its_items_are_read_through_its_query() {
    let mut steam = steam();
    let (query, call) = sent(&mut steam);
    fake(|f| {
        f.details = vec![
            details(100, b"Castle", b"maps,pvp"),
            details(101, b"Keep", b""),
        ];
    });
    answer::<QueryPage>(&mut steam, query_completed(7, 1, 2, 120));
    let page = ready(&mut steam, call);
    assert_eq!(
        (page.result, page.returned, page.total, page.cached),
        (EResult::OK, 2, 120, true)
    );
    let items = steam.workshop().results(&query, &page).unwrap();
    assert_eq!(
        logged("GetQueryUGCResult"),
        ["GetQueryUGCResult|7|0", "GetQueryUGCResult|7|1"]
    );
    assert_eq!(items.len(), 2);
    let first = &items[0];
    assert_eq!(first.item, ItemId(100));
    assert_eq!(first.title, "Castle");
    assert_eq!(first.tags, ["maps", "pvp"]);
    assert_eq!(first.file_type, FileType::GAME_MANAGED_ITEM);
    assert_eq!(first.visibility, Visibility::Private);
    assert_eq!(first.owner, SteamId(testing::STEAM_ID));
    assert!(first.accepted && !first.banned);
    assert!((first.score - 0.5).abs() < f32::EPSILON);
    assert_eq!((first.votes_up, first.total_size), (7, 4096));
    assert!(items[1].tags.is_empty(), "no tags is no empty tag");
    assert_eq!(steam.diagnostics().lossy_strings, 0);
}

/// **Paging**: a page of [`UGC_RESULTS_PER_PAGE`] names the next until the
/// pages seen cover every match.
#[test]
fn paging_stops_at_the_last_match() {
    let page = |total| QueryPage {
        handle: 7,
        result: EResult::OK,
        returned: 50,
        total,
        cached: false,
    };
    assert_eq!(page(120).next_page(1), Some(2));
    assert_eq!(page(120).next_page(2), Some(3));
    assert_eq!(page(120).next_page(3), None);
    assert_eq!(page(100).next_page(2), None, "exactly two full pages");
    assert_eq!(page(101).next_page(2), Some(3));
    assert_eq!(page(0).next_page(1), None);
    assert_eq!(page(u32::MAX).next_page(u32::MAX), None, "no page past u32");
}

#[test]
fn a_page_of_another_query_or_a_failed_page_is_refused() {
    let mut steam = steam();
    let (query, call) = sent(&mut steam);
    answer::<QueryPage>(&mut steam, query_completed(8, 1, 1, 1));
    let foreign = ready(&mut steam, call);
    assert_eq!(
        steam.workshop().results(&query, &foreign).unwrap_err(),
        SteamError::OutOfRange("page")
    );
    let failed = QueryPage {
        handle: 7,
        result: EResult::FAIL,
        returned: 1,
        total: 1,
        cached: false,
    };
    assert_eq!(
        steam.workshop().results(&query, &failed).unwrap_err(),
        SteamError::Result(EResult::FAIL)
    );
    assert!(logged("GetQueryUGCResult").is_empty());
}

#[test]
fn an_item_steam_will_not_hand_over_is_refused() {
    let mut steam = steam();
    let (query, _call) = sent(&mut steam);
    fake(|f| f.details = vec![details(1, b"a", b""), details(2, b"b", b"")]);
    let page = QueryPage {
        handle: 7,
        result: EResult::OK,
        returned: 3,
        total: 3,
        cached: false,
    };
    assert_eq!(
        steam.workshop().results(&query, &page).unwrap_err(),
        SteamError::Refused("GetQueryUGCResult")
    );
}

/// **The update-handle state machine**: opened on an item, each change
/// staged on its handle, submitted once — consuming it — and then only its
/// progress read, on the same handle.
#[test]
fn an_update_stages_on_its_handle_then_submits_and_reports_progress() {
    let mut steam = steam();
    fake(|f| {
        f.update = 9;
        f.call = 77;
        f.progress = (3, 512, 2048);
    });
    let mut update = steam.workshop().start_update(ItemId(5)).unwrap();
    assert_eq!(update.item(), ItemId(5));
    update.set_title("Castle").unwrap();
    update.set_description("A keep.").unwrap();
    update.set_metadata("v=2").unwrap();
    update.set_visibility(Visibility::FriendsOnly).unwrap();
    update.set_tags(&["maps", "pvp"]).unwrap();
    let root = std::env::temp_dir();
    update.set_content(&root).unwrap();
    update.set_preview(&root.join("preview.png")).unwrap();
    let (call, submission) = steam.workshop().submit(update, "first").unwrap();
    assert_eq!(submission.item(), ItemId(5));
    assert_eq!(
        submission.progress(),
        UpdateProgress {
            status: UpdateStatus::UploadingContent,
            processed: 512,
            total: 2048,
        }
    );
    let root = root.to_str().unwrap().to_owned();
    assert_eq!(
        fake(|f| f.log.clone()),
        [
            "StartItemUpdate|480|5".to_owned(),
            "SetItemTitle|9|Castle".to_owned(),
            "SetItemDescription|9|A keep.".to_owned(),
            "SetItemMetadata|9|v=2".to_owned(),
            "SetItemVisibility|9|1".to_owned(),
            "SetItemTags|9|maps,pvp|false".to_owned(),
            format!("SetItemContent|9|{root}"),
            format!(
                "SetItemPreview|9|{}",
                Path::new(&root).join("preview.png").display()
            ),
            "SubmitItemUpdate|9|first".to_owned(),
            "GetItemUpdateProgress|9".to_owned(),
        ]
    );
    let mut raw = payload::<structs::SubmitItemUpdateResult>(&[]);
    raw[..4].copy_from_slice(&1_i32.to_le_bytes());
    raw[4] = 1;
    raw[8..16].copy_from_slice(&5_u64.to_le_bytes());
    answer::<ItemSubmitted>(&mut steam, raw);
    assert_eq!(
        ready(&mut steam, call),
        ItemSubmitted {
            result: EResult::OK,
            item: ItemId(5),
            needs_agreement: true,
        }
    );
    fake(|f| f.progress = (0, 0, 0));
    assert_eq!(submission.progress().status, UpdateStatus::Invalid);
}

#[test]
fn an_update_steam_will_not_open_or_submit_is_refused() {
    let mut steam = steam();
    fake(|f| f.update = u64::MAX);
    assert_eq!(
        steam.workshop().start_update(ItemId(5)).unwrap_err(),
        SteamError::Refused("StartItemUpdate")
    );
    fake(|f| f.update = 9);
    let update = steam.workshop().start_update(ItemId(5)).unwrap();
    assert_eq!(
        steam.workshop().submit(update, "note").unwrap_err(),
        SteamError::Refused("SubmitItemUpdate")
    );
    fake(|f| f.update = 9);
    let mut update = steam.workshop().start_update(ItemId(5)).unwrap();
    testing::script(|s| s.refuse = true);
    assert_eq!(
        update.set_title("x").unwrap_err(),
        SteamError::Refused("SetItemTitle")
    );
}

/// Every limit is checked before the call, at exactly the header's value.
#[test]
fn each_limit_is_refused_before_the_call_and_the_limit_itself_passes() {
    let mut steam = steam();
    fake(|f| f.update = 9);
    let mut update = steam.workshop().start_update(ItemId(5)).unwrap();
    let before = fake(|f| f.log.len());
    let too_long = |argument, len, max| SteamError::TooLong { argument, len, max };
    assert_eq!(
        update
            .set_title(&"t".repeat(MAX_ITEM_TITLE_LENGTH + 1))
            .unwrap_err(),
        too_long("title", 129, 128)
    );
    assert_eq!(
        update
            .set_description(&"d".repeat(MAX_ITEM_DESCRIPTION_LENGTH + 1))
            .unwrap_err(),
        too_long("description", 8000, 7999)
    );
    assert_eq!(
        update
            .set_metadata(&"m".repeat(MAX_ITEM_METADATA_LENGTH + 1))
            .unwrap_err(),
        too_long("metadata", 10000, 9999)
    );
    assert_eq!(
        update
            .set_tags(&[&"g".repeat(MAX_ITEM_TAG_LENGTH + 1)])
            .unwrap_err(),
        too_long("tag", 256, 255)
    );
    assert_eq!(
        update.set_tags(&["a,b"]).unwrap_err(),
        SteamError::OutOfRange("tag")
    );
    assert_eq!(
        update.set_content(Path::new("relative/dir")).unwrap_err(),
        SteamError::OutOfRange("content folder")
    );
    assert_eq!(
        update.set_preview(Path::new("preview.png")).unwrap_err(),
        SteamError::OutOfRange("preview file")
    );
    assert_eq!(fake(|f| f.log.len()), before, "nothing reached Steam");
    update
        .set_title(&"t".repeat(MAX_ITEM_TITLE_LENGTH))
        .unwrap();
    update
        .set_description(&"d".repeat(MAX_ITEM_DESCRIPTION_LENGTH))
        .unwrap();
    update
        .set_metadata(&"m".repeat(MAX_ITEM_METADATA_LENGTH))
        .unwrap();
    update
        .set_tags(&[&"g".repeat(MAX_ITEM_TAG_LENGTH)])
        .unwrap();
    assert_eq!(
        steam
            .workshop()
            .submit(update, &"n".repeat(MAX_CHANGE_NOTE_LENGTH + 1))
            .unwrap_err(),
        too_long("change note", 8000, 7999)
    );
    assert!(logged("SubmitItemUpdate").is_empty());
}

/// **Install-folder string growth**: a folder reaching the buffer's last byte
/// but one is read again in a larger buffer, and past the ceiling it is
/// `Truncated`, never cut.
#[test]
fn the_install_folder_grows_until_it_fits() {
    let steam_ = &mut steam();
    assert_eq!(steam_.workshop().install_info(ItemId(5)), Ok(None));
    let long = vec![b'a'; 300];
    fake(|f| {
        f.install_offered.clear();
        f.install = Some((1234, long.clone(), 99));
    });
    assert_eq!(
        steam_.workshop().install_info(ItemId(5)),
        Ok(Some(InstallInfo {
            size_on_disk: 1234,
            folder: PathBuf::from("a".repeat(300)),
            updated: 99,
        }))
    );
    assert_eq!(fake(|f| f.install_offered.clone()), [256, 512]);
    // 255 bytes reach a 256-byte buffer's last byte but one: read again.
    fake(|f| {
        f.install_offered.clear();
        f.install = Some((1, vec![b'b'; 255], 1));
    });
    let info = steam_.workshop().install_info(ItemId(5)).unwrap().unwrap();
    assert_eq!(info.folder.as_os_str().len(), 255);
    assert_eq!(fake(|f| f.install_offered.clone()), [256, 512]);
    fake(|f| {
        f.install_offered.clear();
        f.install = Some((1, vec![b'c'; 254], 1));
    });
    steam_.workshop().install_info(ItemId(5)).unwrap();
    assert_eq!(fake(|f| f.install_offered.clone()), [256]);
    fake(|f| f.install = Some((1, vec![b'd'; crate::MAX_TEXT_BYTES], 1)));
    assert_eq!(
        steam_.workshop().install_info(ItemId(5)),
        Err(SteamError::Truncated("GetItemInstallInfo"))
    );
}

#[test]
fn subscribed_items_are_read_and_never_past_their_buffer() {
    let steam_ = &mut steam();
    fake(|f| f.subscribed = vec![1, 2, 3]);
    assert_eq!(
        steam_.workshop().subscribed_items(true),
        [ItemId(1), ItemId(2), ItemId(3)]
    );
    // Steam counts one, then claims to have written three.
    fake(|f| f.count = Some(1));
    assert_eq!(steam_.workshop().subscribed_items(false), [ItemId(1)]);
    assert_eq!(
        logged("GetSubscribedItems"),
        ["GetSubscribedItems|3|true", "GetSubscribedItems|1|false"]
    );
}

#[test]
fn state_download_progress_and_download_are_steams_answers() {
    let steam_ = &mut steam();
    fake(|f| f.state = 1 | 4 | 8);
    let state = steam_.workshop().state(ItemId(5));
    assert!(state.contains(ItemState::SUBSCRIBED));
    assert!(
        state.contains(ItemState(4 | 8)),
        "installed and out of date"
    );
    assert!(!state.contains(ItemState::DOWNLOADING));
    assert_eq!(steam_.workshop().download_progress(ItemId(5)), None);
    fake(|f| f.download_info = Some((10, 40)));
    assert_eq!(
        steam_.workshop().download_progress(ItemId(5)),
        Some(DownloadProgress {
            downloaded: 10,
            total: 40,
        })
    );
    steam_.workshop().download(ItemId(5), true).unwrap();
    testing::script(|s| s.refuse = true);
    assert_eq!(
        steam_.workshop().download(ItemId(5), false),
        Err(SteamError::Refused("DownloadItem"))
    );
    assert_eq!(
        logged("DownloadItem"),
        ["DownloadItem|5|true", "DownloadItem|5|false"]
    );
}

/// `result`, then an 8-byte item id at the offset this OS packs it to.
fn result_and_item<T>(offset: usize, result: i32, item: u64) -> Vec<u8> {
    payload::<T>(&[(0, &result.to_le_bytes()), (offset, &item.to_le_bytes())])
}

#[test]
fn subscribe_unsubscribe_create_and_delete_answer_their_rows() {
    use core::mem::offset_of;
    let mut steam = steam();
    fake(|f| f.call = 77);

    let call = steam.workshop().subscribe(ItemId(5)).unwrap();
    answer::<Subscribed>(
        &mut steam,
        result_and_item::<structs::SubscribeResult>(
            offset_of!(structs::SubscribeResult, item),
            1,
            5,
        ),
    );
    assert_eq!(
        ready(&mut steam, call),
        Subscribed {
            result: EResult::OK,
            item: ItemId(5),
        }
    );

    let call = steam.workshop().unsubscribe(ItemId(6)).unwrap();
    answer::<Unsubscribed>(
        &mut steam,
        result_and_item::<structs::UnsubscribeResult>(
            offset_of!(structs::UnsubscribeResult, item),
            2,
            6,
        ),
    );
    assert_eq!(
        ready(&mut steam, call),
        Unsubscribed {
            result: EResult::FAIL,
            item: ItemId(6),
        }
    );

    let call = steam.workshop().create_item(FileType::COMMUNITY).unwrap();
    let mut created = result_and_item::<structs::CreateItemResult>(
        offset_of!(structs::CreateItemResult, item),
        1,
        8,
    );
    created[offset_of!(structs::CreateItemResult, needs_agreement)] = 1;
    answer::<ItemCreated>(&mut steam, created);
    assert_eq!(
        ready(&mut steam, call),
        ItemCreated {
            result: EResult::OK,
            item: ItemId(8),
            needs_agreement: true,
        }
    );

    let call = steam.workshop().delete_item(ItemId(8)).unwrap();
    answer::<ItemDeleted>(
        &mut steam,
        result_and_item::<structs::DeleteItemResult>(
            offset_of!(structs::DeleteItemResult, item),
            1,
            8,
        ),
    );
    assert_eq!(
        ready(&mut steam, call),
        ItemDeleted {
            result: EResult::OK,
            item: ItemId(8),
        }
    );
    assert_eq!(
        fake(|f| f.log.clone()),
        [
            "SubscribeItem|5",
            "UnsubscribeItem|6",
            "CreateItem|480|0",
            "DeleteItem|8"
        ]
    );
}

#[test]
fn a_call_steam_does_not_start_is_refused_naming_it() {
    let mut steam = steam();
    let mut workshop = steam.workshop();
    assert_eq!(
        workshop.subscribe(ItemId(1)).unwrap_err(),
        SteamError::Refused("SubscribeItem")
    );
    assert_eq!(
        workshop.unsubscribe(ItemId(1)).unwrap_err(),
        SteamError::Refused("UnsubscribeItem")
    );
    assert_eq!(
        workshop.create_item(FileType::COMMUNITY).unwrap_err(),
        SteamError::Refused("CreateItem")
    );
    assert_eq!(
        workshop.delete_item(ItemId(1)).unwrap_err(),
        SteamError::Refused("DeleteItem")
    );
}

#[test]
fn installs_and_finished_downloads_arrive_as_events() {
    use core::mem::offset_of;
    use structs::{DownloadItemResult as D, ItemInstalled as I};
    let mut steam = steam();
    let installed = payload::<I>(&[
        (offset_of!(I, app), &480_u32.to_le_bytes()),
        (offset_of!(I, item), &5_u64.to_le_bytes()),
    ]);
    let downloaded = payload::<D>(&[
        (offset_of!(D, app), &480_u32.to_le_bytes()),
        (offset_of!(D, item), &6_u64.to_le_bytes()),
        (offset_of!(D, result), &1_i32.to_le_bytes()),
    ]);
    testing::script(|s| {
        s.queue.push_back(FakeMsg::payload(3405, installed));
        s.queue.push_back(FakeMsg::payload(3406, downloaded));
    });
    steam.pump();
    assert_eq!(
        steam.events().collect::<Vec<_>>(),
        [
            SteamEvent::WorkshopItemInstalled {
                app: AppId(480),
                item: ItemId(5),
            },
            SteamEvent::WorkshopItemDownloaded {
                app: AppId(480),
                item: ItemId(6),
                result: EResult::OK,
            },
        ]
    );
    assert_eq!(steam.diagnostics().decode_mismatches, 0);
}

/// Each answer and event under its header id — spelled out, since every
/// other test here builds its completion from the row it checks.
#[test]
fn the_workshop_answers_and_events_have_valves_ids() {
    use crate::call::private::Answer;
    assert_eq!(<QueryPage as Answer>::ROW.id(), 3401);
    assert_eq!(<ItemCreated as Answer>::ROW.id(), 3403);
    assert_eq!(<ItemSubmitted as Answer>::ROW.id(), 3404);
    assert_eq!(<ItemDeleted as Answer>::ROW.id(), 3417);
    assert_eq!(<Subscribed as Answer>::ROW.id(), 1313);
    assert_eq!(<Unsubscribed as Answer>::ROW.id(), 1315);
    assert_eq!(
        crate::callbacks::find(3405).map(|row| row.name),
        Some("ItemInstalled_t")
    );
    assert_eq!(
        crate::callbacks::find(3406).map(|row| row.name),
        Some("DownloadItemResult_t")
    );
}
