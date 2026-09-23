//! Stats and achievements over the fake, and the pump's readiness rule.

use core::mem::offset_of;

use super::*;
use crate::{
    AppId, EResult, SteamEvent, SteamId,
    client::init_on,
    ffi::structs::{UserAchievementStored, UserStatsReceived, UserStatsStored},
    testing::{self, FakeMsg, payload, script},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

/// A `UserStatsReceived_t` for `user` and `game`.
fn received(game: u64, user: u64, result: i32) -> FakeMsg {
    FakeMsg::payload(
        1101,
        payload::<UserStatsReceived>(&[
            (offset_of!(UserStatsReceived, game_id), &game.to_le_bytes()),
            (offset_of!(UserStatsReceived, result), &result.to_le_bytes()),
            (offset_of!(UserStatsReceived, user), &user.to_le_bytes()),
        ]),
    )
}

/// A `Steam` whose local user's stats have arrived, holding one achievement
/// and one stat of each kind.
fn ready() -> Steam {
    let mut steam = steam();
    script(|s| {
        s.queue.push_back(received(480, testing::STEAM_ID, 1));
        s.stats
            .achievements
            .insert("ACH_WIN_ONE_GAME".into(), (false, 0));
        s.stats.ints.insert("NumGames".into(), 3);
        s.stats.floats.insert("FeetTraveled".into(), 1.5);
    });
    steam.pump();
    assert!(steam.stats().ready());
    steam.events().for_each(drop);
    steam
}

#[test]
fn every_call_before_the_stats_arrive_is_a_typed_error_and_calls_nothing() {
    let steam = steam();
    let stats = steam.stats();
    assert!(!stats.ready());
    assert_eq!(stats.store(), Err(SteamError::StatsNotReady));
    assert_eq!(
        stats.set_achievement("ACH_WIN_ONE_GAME"),
        Err(SteamError::StatsNotReady)
    );
    assert_eq!(stats.set_i32("NumGames", 1), Err(SteamError::StatsNotReady));
    assert_eq!(stats.i32("NumGames"), Err(SteamError::StatsNotReady));
    assert_eq!(
        stats.achievement("ACH_WIN_ONE_GAME"),
        Err(SteamError::StatsNotReady)
    );
    assert_eq!(script(|s| s.stats.calls), 0);
}

#[test]
fn only_the_local_users_stats_for_this_game_make_them_ready() {
    let mut steam = steam();
    script(|s| {
        s.queue.push_back(received(480, testing::STEAM_ID + 1, 1));
        s.queue.push_back(received(481, testing::STEAM_ID, 1));
        s.queue.push_back(received(480, testing::STEAM_ID, 2));
    });
    steam.pump();
    assert!(!steam.stats().ready());
    let events: Vec<_> = steam.events().collect();
    assert_eq!(
        events,
        [
            SteamEvent::StatsReceived {
                user: SteamId(testing::STEAM_ID + 1),
                result: EResult::OK
            },
            SteamEvent::StatsReceived {
                user: SteamId(testing::STEAM_ID),
                result: EResult(2)
            },
        ],
        "another game's arrival is not this game's event"
    );
    assert_eq!(steam.diagnostics().unknown, 1);

    script(|s| s.queue.push_back(received(480, testing::STEAM_ID, 1)));
    steam.pump();
    assert!(steam.stats().ready());
}

#[test]
fn achievements_and_stats_read_and_write_through_steam() {
    let steam = ready();
    let stats = steam.stats();
    assert_eq!(
        stats.achievement("ACH_WIN_ONE_GAME"),
        Ok(Achieved {
            unlocked: false,
            unlock_time: None
        })
    );
    stats.set_achievement("ACH_WIN_ONE_GAME").unwrap();
    assert_eq!(
        stats.achievement("ACH_WIN_ONE_GAME"),
        Ok(Achieved {
            unlocked: true,
            unlock_time: Some(1_700_000_000)
        })
    );
    stats.clear_achievement("ACH_WIN_ONE_GAME").unwrap();
    assert!(!stats.achievement("ACH_WIN_ONE_GAME").unwrap().unlocked);

    assert_eq!(stats.i32("NumGames"), Ok(3));
    stats.set_i32("NumGames", 4).unwrap();
    assert_eq!(stats.i32("NumGames"), Ok(4));
    assert_eq!(stats.f32("FeetTraveled"), Ok(1.5));
    stats.set_f32("FeetTraveled", 2.5).unwrap();
    assert_eq!(stats.f32("FeetTraveled"), Ok(2.5));

    stats.store().unwrap();
    assert_eq!(script(|s| s.stats.stores), 1);
}

#[test]
fn a_name_steam_does_not_know_is_refused_naming_the_call() {
    let steam = ready();
    let stats = steam.stats();
    assert_eq!(
        stats.set_achievement("NOPE"),
        Err(SteamError::Refused("SetAchievement"))
    );
    assert_eq!(
        stats.achievement("NOPE"),
        Err(SteamError::Refused("GetAchievementAndUnlockTime"))
    );
    assert_eq!(stats.i32("Nope"), Err(SteamError::Refused("GetStatInt32")));
    script(|s| s.refuse = true);
    assert_eq!(stats.store(), Err(SteamError::Refused("StoreStats")));
}

#[test]
fn a_name_steam_cannot_take_is_refused_before_the_call() {
    let steam = ready();
    let before = script(|s| s.stats.calls);
    let long = "A".repeat(MAX_STAT_NAME_LENGTH + 1);
    assert!(matches!(
        steam.stats().set_achievement(&long),
        Err(SteamError::TooLong { .. })
    ));
    assert!(matches!(
        steam.stats().set_i32("Num\0Games", 1),
        Err(SteamError::InteriorNul(_))
    ));
    assert_eq!(script(|s| s.stats.calls), before);
}

#[test]
fn stored_and_achievement_events_decode_for_this_game_only() {
    let mut steam = steam();
    let mut name = [0_u8; 128];
    name[..16].copy_from_slice(b"ACH_WIN_ONE_GAME");
    let stored = |game: u64| {
        FakeMsg::payload(
            1102,
            payload::<UserStatsStored>(&[
                (offset_of!(UserStatsStored, game_id), &game.to_le_bytes()),
                (offset_of!(UserStatsStored, result), &1_i32.to_le_bytes()),
            ]),
        )
    };
    let achievement = |current: u32, max: u32| {
        FakeMsg::payload(
            1103,
            payload::<UserAchievementStored>(&[
                (
                    offset_of!(UserAchievementStored, game_id),
                    &480_u64.to_le_bytes(),
                ),
                (offset_of!(UserAchievementStored, name), &name),
                (
                    offset_of!(UserAchievementStored, current),
                    &current.to_le_bytes(),
                ),
                (offset_of!(UserAchievementStored, max), &max.to_le_bytes()),
            ]),
        )
    };
    script(|s| {
        s.queue.push_back(stored(480));
        s.queue.push_back(stored(999));
        s.queue.push_back(achievement(0, 0));
        s.queue.push_back(achievement(3, 10));
    });
    steam.pump();
    let events: Vec<_> = steam.events().collect();
    assert_eq!(
        events,
        [
            SteamEvent::StatsStored {
                result: EResult::OK
            },
            SteamEvent::AchievementStored {
                name: "ACH_WIN_ONE_GAME".into(),
                progress: None
            },
            SteamEvent::AchievementStored {
                name: "ACH_WIN_ONE_GAME".into(),
                progress: Some((3, 10))
            },
        ]
    );
    assert_eq!(steam.diagnostics().decode_mismatches, 0);
}
