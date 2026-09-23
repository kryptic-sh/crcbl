//! Leaderboards over the fake: each call registered, each answer decoded.

use core::mem::offset_of;

use super::*;
use crate::{
    AppId, CallResult, CallState,
    client::init_on,
    ffi::structs::{LeaderboardFindResult, LeaderboardScoreUploaded, LeaderboardScoresDownloaded},
    testing::{self, completion, payload, script},
};

/// The board every test uses.
const BOARD: u64 = 0x00AB_CDEF;
/// The downloaded entries' handle.
const ENTRIES: u64 = 0x0E17_0000;

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

/// Answers call `handle` with `bytes` at the next pump, and takes it.
fn answer<T: CallResult + core::fmt::Debug>(
    steam: &mut Steam,
    call: SteamCall<T>,
    handle: u64,
    bytes: Vec<u8>,
) -> T {
    let row = T::ROW;
    script(|s| {
        s.results.push((handle, bytes, false));
        s.queue.push_back(completion(handle, row.id(), row.size));
    });
    steam.pump();
    match steam.take(call) {
        CallState::Ready(answer) => answer,
        other => panic!("not answered: {other:?}"),
    }
}

fn found(board: u64, found: u8) -> Vec<u8> {
    payload::<LeaderboardFindResult>(&[
        (
            offset_of!(LeaderboardFindResult, leaderboard),
            &board.to_le_bytes(),
        ),
        (offset_of!(LeaderboardFindResult, found), &[found]),
    ])
}

#[test]
fn find_or_create_passes_the_sort_and_display_and_answers_the_board() {
    let mut steam = steam();
    script(|s| s.next_call = 21);
    let call = steam
        .leaderboards()
        .find_or_create(
            "Feet Traveled",
            LeaderboardSort::Descending,
            LeaderboardDisplay::TimeMilliseconds,
        )
        .unwrap();
    assert_eq!(
        script(|s| s.stats.finds.clone()),
        [("Feet Traveled".to_owned(), 2, 3)]
    );
    let answer = answer(&mut steam, call, 21, found(BOARD, 1));
    assert_eq!(answer.leaderboard(), Some(Leaderboard(BOARD)));
}

#[test]
fn a_board_not_found_is_none() {
    let mut steam = steam();
    script(|s| s.next_call = 22);
    let call = steam.leaderboards().find("Missing").unwrap();
    assert_eq!(
        answer(&mut steam, call, 22, found(0, 0)).leaderboard(),
        None
    );
}

#[test]
fn a_call_steam_does_not_start_is_refused() {
    let mut steam = steam();
    script(|s| s.next_call = 0);
    assert_eq!(
        steam.leaderboards().find("Missing").err(),
        Some(SteamError::Refused("FindLeaderboard"))
    );
    assert_eq!(
        steam
            .leaderboards()
            .download(Leaderboard(BOARD), Range::Friends)
            .err(),
        Some(SteamError::Refused("DownloadLeaderboardEntries"))
    );
}

#[test]
fn an_upload_carries_its_details_and_answers_the_new_rank() {
    let mut steam = steam();
    script(|s| s.next_call = 23);
    let call = steam
        .leaderboards()
        .upload(Leaderboard(BOARD), UploadMethod::KeepBest, 900, &[7, 8])
        .unwrap();
    assert_eq!(
        script(|s| s.stats.uploads.clone()),
        [(BOARD, 1, 900, vec![7, 8])]
    );
    let bytes = payload::<LeaderboardScoreUploaded>(&[
        (offset_of!(LeaderboardScoreUploaded, success), &[1]),
        (
            offset_of!(LeaderboardScoreUploaded, leaderboard),
            &BOARD.to_le_bytes(),
        ),
        (
            offset_of!(LeaderboardScoreUploaded, score),
            &900_i32.to_le_bytes(),
        ),
        (offset_of!(LeaderboardScoreUploaded, changed), &[1]),
        (
            offset_of!(LeaderboardScoreUploaded, rank_new),
            &4_i32.to_le_bytes(),
        ),
        (
            offset_of!(LeaderboardScoreUploaded, rank_previous),
            &0_i32.to_le_bytes(),
        ),
    ]);
    assert_eq!(
        answer(&mut steam, call, 23, bytes),
        ScoreUploaded {
            success: true,
            score: 900,
            changed: true,
            rank: 4,
            previous_rank: None,
        }
    );
}

#[test]
fn more_details_than_an_entry_holds_are_refused_before_the_call() {
    let mut steam = steam();
    script(|s| s.next_call = 24);
    let details = [0_i32; MAX_LEADERBOARD_DETAILS + 1];
    assert_eq!(
        steam
            .leaderboards()
            .upload(Leaderboard(BOARD), UploadMethod::ForceUpdate, 1, &details)
            .err(),
        Some(SteamError::TooMany {
            what: "leaderboard details",
            max: MAX_LEADERBOARD_DETAILS
        })
    );
    assert_eq!(script(|s| s.stats.calls), 0);
    let at_the_limit = steam.leaderboards().upload(
        Leaderboard(BOARD),
        UploadMethod::ForceUpdate,
        1,
        &details[..MAX_LEADERBOARD_DETAILS],
    );
    assert!(at_the_limit.is_ok());
    assert_eq!(
        script(|s| s.stats.uploads[0].3.len()),
        MAX_LEADERBOARD_DETAILS
    );
}

#[test]
fn downloaded_entries_are_read_one_by_one_at_the_pump() {
    let mut steam = steam();
    script(|s| {
        s.next_call = 25;
        s.stats.entries_handle = ENTRIES;
        s.stats.entries = vec![
            (111, 1, 5000, vec![1, 2, 3]),
            (222, 2, 4000, Vec::new()),
            // More details than any entry may hold: kept to the limit.
            (333, 3, 3000, vec![9; MAX_LEADERBOARD_DETAILS + 5]),
        ];
        s.stats.refuse_entry = Some(1);
    });
    let call = steam
        .leaderboards()
        .download(
            Leaderboard(BOARD),
            Range::AroundUser {
                before: -5,
                after: 5,
            },
        )
        .unwrap();
    assert_eq!(script(|s| s.stats.downloads.clone()), [(BOARD, 1, -5, 5)]);
    let bytes = payload::<LeaderboardScoresDownloaded>(&[
        (
            offset_of!(LeaderboardScoresDownloaded, leaderboard),
            &BOARD.to_le_bytes(),
        ),
        (
            offset_of!(LeaderboardScoresDownloaded, entries),
            &ENTRIES.to_le_bytes(),
        ),
        (
            offset_of!(LeaderboardScoresDownloaded, count),
            &3_i32.to_le_bytes(),
        ),
    ]);
    let entries = answer(&mut steam, call, 25, bytes);
    assert_eq!(entries.leaderboard, Leaderboard(BOARD));
    assert_eq!(
        entries.entries,
        [
            Entry {
                user: SteamId(111),
                rank: 1,
                score: 5000,
                details: vec![1, 2, 3],
            },
            Entry {
                user: SteamId(333),
                rank: 3,
                score: 3000,
                details: vec![9; MAX_LEADERBOARD_DETAILS],
            },
        ],
        "the refused entry is left out"
    );
    assert!(
        script(|s| s.stats.details_offered.clone())
            .iter()
            .all(|&offered| offered == i32::try_from(MAX_LEADERBOARD_DETAILS).unwrap())
    );
}

#[test]
fn global_and_friends_ranges_are_steams_requests() {
    assert_eq!(Range::Global { first: 1, last: 10 }.raw(), (0, 1, 10));
    assert_eq!(
        Range::AroundUser {
            before: -2,
            after: 3
        }
        .raw(),
        (1, -2, 3)
    );
    assert_eq!(Range::Friends.raw(), (2, 0, 0));
}

#[test]
fn a_long_name_is_refused_before_the_call() {
    let mut steam = steam();
    let long = "L".repeat(MAX_LEADERBOARD_NAME_LENGTH + 1);
    assert!(matches!(
        steam.leaderboards().find(&long),
        Err(SteamError::TooLong { .. })
    ));
    assert_eq!(script(|s| s.stats.calls), 0);
}
