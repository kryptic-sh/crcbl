//! Achievements, stats and leaderboards against a real client, under app 480
//! (SpaceWar), through the public API only. Never run by CI: `#[ignore]`d, and
//! run by hand as `tests/smoke.rs` is:
//!
//! ```text
//! cargo test -p crcbl-steam --test stats_smoke -- --ignored --nocapture
//! # also upload a score (KeepBest, so an existing better one stays):
//! CRCBL_STEAM_UPLOAD=1 cargo test -p crcbl-steam --test stats_smoke -- --ignored --nocapture
//! ```
//!
//! It waits for the stats, unlocks `ACH_WIN_ONE_GAME` — Steam's toast should
//! appear — and then puts it back as it found it, since 480 is shared by every
//! Steamworks developer. It reads `NumGames` and downloads the entries around
//! the local user on "Feet Traveled". Those names are SpaceWar's, from the
//! Steamworks example, and **believed** to exist on 480; this is where that is
//! checked. Nothing is asserted about their values.

#![cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]

use std::time::{Duration, Instant};

use crcbl_steam::{
    AppId, CallState, LeaderboardDisplay, LeaderboardSort, Range, Steam, SteamEvent, UploadMethod,
};

/// How long to wait for any one answer.
const PATIENCE: Duration = Duration::from_secs(30);
const FRAME: Duration = Duration::from_millis(16);

/// Pumps until `ready` answers, printing every event.
fn until<T>(
    steam: &mut Steam,
    what: &str,
    mut ready: impl FnMut(&mut Steam, &[SteamEvent]) -> Option<T>,
) -> T {
    let start = Instant::now();
    loop {
        steam.pump();
        let events: Vec<_> = steam.events().collect();
        for event in &events {
            println!("{event:?}");
        }
        if let Some(value) = ready(steam, &events) {
            return value;
        }
        assert!(start.elapsed() < PATIENCE, "timed out waiting for {what}");
        std::thread::sleep(FRAME);
    }
}

fn stored(steam: &mut Steam) {
    steam.stats().store().expect("StoreStats");
    until(steam, "StatsStored", |_, events| {
        events
            .iter()
            .any(|event| matches!(event, SteamEvent::StatsStored { .. }))
            .then_some(())
    });
}

#[test]
#[ignore = "needs a running Steam client, the redistributable and steam_appid.txt"]
fn unlock_and_restore_an_achievement_and_read_a_leaderboard_under_480() {
    let mut steam = Steam::init(AppId(480)).unwrap_or_else(|err| panic!("Steam::init: {err}"));
    until(&mut steam, "the stats", |steam, _| {
        steam.stats().ready().then_some(())
    });

    let before = steam
        .stats()
        .achievement("ACH_WIN_ONE_GAME")
        .expect("SpaceWar's ACH_WIN_ONE_GAME");
    println!("ACH_WIN_ONE_GAME before: {before:?}");
    println!("NumGames: {:?}", steam.stats().i32("NumGames"));
    steam
        .stats()
        .set_achievement("ACH_WIN_ONE_GAME")
        .expect("SetAchievement");
    stored(&mut steam);
    assert!(
        steam
            .stats()
            .achievement("ACH_WIN_ONE_GAME")
            .expect("read")
            .unlocked
    );
    if !before.unlocked {
        steam
            .stats()
            .clear_achievement("ACH_WIN_ONE_GAME")
            .expect("ClearAchievement");
        stored(&mut steam);
    }

    let mut call = Some(
        steam
            .leaderboards()
            .find_or_create(
                "Feet Traveled",
                LeaderboardSort::Descending,
                LeaderboardDisplay::Numeric,
            )
            .expect("FindOrCreateLeaderboard"),
    );
    let board = until(&mut steam, "the leaderboard", |steam, _| {
        match steam.take(call.take().expect("once per frame")) {
            CallState::Pending(pending) => {
                call = Some(pending);
                None
            }
            CallState::Ready(found) => Some(found.leaderboard().expect("Feet Traveled exists")),
            CallState::Failed(error) => panic!("FindOrCreateLeaderboard failed: {error}"),
        }
    });

    if std::env::var_os("CRCBL_STEAM_UPLOAD").is_some() {
        let mut upload = Some(
            steam
                .leaderboards()
                .upload(board, UploadMethod::KeepBest, 1, &[])
                .expect("UploadLeaderboardScore"),
        );
        let uploaded = until(&mut steam, "the upload", |steam, _| {
            match steam.take(upload.take().expect("once per frame")) {
                CallState::Pending(pending) => {
                    upload = Some(pending);
                    None
                }
                CallState::Ready(uploaded) => Some(uploaded),
                CallState::Failed(error) => panic!("UploadLeaderboardScore failed: {error}"),
            }
        });
        println!("{uploaded:?}");
    }

    let mut download = Some(
        steam
            .leaderboards()
            .download(
                board,
                Range::AroundUser {
                    before: -5,
                    after: 5,
                },
            )
            .expect("DownloadLeaderboardEntries"),
    );
    let entries = until(&mut steam, "the entries", |steam, _| {
        match steam.take(download.take().expect("once per frame")) {
            CallState::Pending(pending) => {
                download = Some(pending);
                None
            }
            CallState::Ready(entries) => Some(entries),
            CallState::Failed(error) => panic!("DownloadLeaderboardEntries failed: {error}"),
        }
    });
    println!("{} entries: {:?}", entries.entries.len(), entries.entries);
    let diagnostics = steam.diagnostics();
    println!("{diagnostics:?}");
    assert_eq!(diagnostics.decode_mismatches, 0);
}
