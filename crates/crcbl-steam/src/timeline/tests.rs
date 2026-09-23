//! The timeline over the fake.

use super::*;
use crate::{
    AppId, CallState,
    client::init_on,
    testing::{self, FakeTimeline, completion},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn fake<R>(f: impl FnOnce(&mut FakeTimeline) -> R) -> R {
    testing::script(|s| f(&mut s.timeline))
}

fn log() -> Vec<String> {
    fake(|f| f.log.clone())
}

const BOSS: TimelineEvent<'static> = TimelineEvent {
    title: "Boss",
    description: "The warden fell",
    icon: "steam_combat",
    priority: 50,
    offset: -2.5,
    clip: ClipPriority::Featured,
};

#[test]
fn the_tooltip_and_game_mode_reach_steam() {
    let mut steam = steam();
    let timeline = steam.timeline();
    timeline.set_tooltip("Floor 3", 0.0).unwrap();
    timeline.clear_tooltip(-1.0).unwrap();
    for mode in [
        GameMode::Playing,
        GameMode::Staging,
        GameMode::Menus,
        GameMode::LoadingScreen,
    ] {
        timeline.set_game_mode(mode);
    }
    assert_eq!(
        log(),
        [
            "tooltip|Floor 3|0",
            "clear tooltip|-1",
            "game mode|1",
            "game mode|2",
            "game mode|3",
            "game mode|4",
        ]
    );
}

#[test]
fn events_carry_every_field_and_the_clip_priority() {
    let mut steam = steam();
    fake(|f| f.event = 77);
    let timeline = steam.timeline();
    assert_eq!(timeline.instant_event(&BOSS), Ok(TimelineEventId(77)));
    assert_eq!(timeline.range_event(&BOSS, 30.0), Ok(TimelineEventId(77)));
    timeline.remove_event(TimelineEventId(77));
    timeline.open_overlay_to_event(TimelineEventId(77));
    for (clip, raw) in [(ClipPriority::None, 1), (ClipPriority::Standard, 2)] {
        timeline
            .instant_event(&TimelineEvent { clip, ..BOSS })
            .unwrap();
        assert!(log().last().unwrap().ends_with(&format!("|{raw}")));
    }
    assert_eq!(
        log()[..4],
        [
            "instant|Boss|The warden fell|steam_combat|50|-2.5|3",
            "range|Boss|The warden fell|steam_combat|50|-2.5|30|3",
            "remove|77",
            "overlay event|77",
        ]
    );
}

/// **Every limit is checked before the call**: a priority past the maximum,
/// a non-finite offset, a duration outside `0..=600`, a NUL — none reaches
/// Steam.
#[test]
fn out_of_range_numbers_and_nuls_are_refused_before_the_call() {
    let mut steam = steam();
    let timeline = steam.timeline();
    let priority = TimelineEvent {
        priority: MAX_TIMELINE_PRIORITY + 1,
        ..BOSS
    };
    assert_eq!(
        timeline.instant_event(&priority),
        Err(SteamError::OutOfRange("priority"))
    );
    for offset in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let event = TimelineEvent { offset, ..BOSS };
        assert_eq!(
            timeline.instant_event(&event),
            Err(SteamError::OutOfRange("offset"))
        );
        assert_eq!(
            timeline.set_tooltip("x", offset),
            Err(SteamError::OutOfRange("offset"))
        );
    }
    for seconds in [-0.1, MAX_TIMELINE_EVENT_SECONDS + 0.1, f32::NAN] {
        assert_eq!(
            timeline.range_event(&BOSS, seconds),
            Err(SteamError::OutOfRange("duration"))
        );
    }
    assert_eq!(
        timeline.instant_event(&TimelineEvent {
            title: "a\0b",
            ..BOSS
        }),
        Err(SteamError::InteriorNul("title"))
    );
    assert_eq!(
        timeline.add_phase_tag("n", "i", "g", MAX_TIMELINE_PRIORITY + 1),
        Err(SteamError::OutOfRange("priority"))
    );
    assert!(log().is_empty(), "{:?}", log());

    let at_the_limits = TimelineEvent {
        priority: MAX_TIMELINE_PRIORITY,
        ..BOSS
    };
    timeline.instant_event(&at_the_limits).unwrap();
    timeline
        .range_event(&at_the_limits, MAX_TIMELINE_EVENT_SECONDS)
        .unwrap();
    timeline.range_event(&at_the_limits, 0.0).unwrap();
}

/// **A range ends exactly once**: on drop, or when ended — and an ended one
/// is not ended again by its drop.
#[test]
fn a_range_ends_once_whether_dropped_or_ended() {
    let mut steam = steam();
    fake(|f| f.event = 5);
    let range = steam.timeline().range_start(&BOSS).unwrap();
    assert_eq!(range.event(), TimelineEventId(5));
    range
        .update(&TimelineEvent {
            title: "Boss II",
            ..BOSS
        })
        .unwrap();
    drop(range);
    let range = steam.timeline().range_start(&BOSS).unwrap();
    range.end(1.5).unwrap();
    let ends: Vec<String> = log().into_iter().filter(|l| l.starts_with("end")).collect();
    assert_eq!(ends, ["end|5|0", "end|5|1.5"]);
    assert!(log().contains(&"update|5|Boss II|The warden fell|steam_combat|50|3".to_owned()));

    let range = steam.timeline().range_start(&BOSS).unwrap();
    assert_eq!(range.end(f32::NAN), Err(SteamError::OutOfRange("offset")));
    assert_eq!(
        log().last().map(String::as_str),
        Some("end|5|0"),
        "ended now"
    );
    let ends = log().iter().filter(|l| l.starts_with("end")).count();
    assert_eq!(ends, 3);
}

#[test]
fn phases_reach_steam_and_ids_are_limited() {
    let mut steam = steam();
    let timeline = steam.timeline();
    timeline.start_phase();
    timeline.set_phase_id("run-12").unwrap();
    timeline
        .add_phase_tag("Warden", "steam_crown", "Bosses", 10)
        .unwrap();
    timeline.set_phase_attribute("Score", "4200", 5).unwrap();
    timeline.open_overlay_to_phase("run-12").unwrap();
    timeline.end_phase();
    assert_eq!(
        log(),
        [
            "start phase",
            "phase id|run-12",
            "phase tag|Warden|steam_crown|Bosses|10",
            "phase attribute|Score|4200|5",
            "overlay phase|run-12",
            "end phase",
        ]
    );
    let long = "x".repeat(MAX_PHASE_ID_LENGTH + 1);
    assert!(matches!(
        timeline.set_phase_id(&long),
        Err(SteamError::TooLong { .. })
    ));
    timeline
        .set_phase_id(&"x".repeat(MAX_PHASE_ID_LENGTH))
        .unwrap();
}

/// The two recording questions are call results, decoded at the pump.
#[test]
fn the_recording_questions_answer_through_the_registry() {
    let mut steam = steam();
    fake(|f| f.call = 900);
    let event_call = steam
        .timeline()
        .event_recording(TimelineEventId(5))
        .unwrap();
    let mut event = 5_u64.to_le_bytes().to_vec();
    event.extend_from_slice(&[1, 0, 0, 0]);
    event.resize(size_of::<structs::SteamTimelineEventRecordingExists>(), 0);
    testing::script(|s| {
        s.results.push((900, event, false));
        s.queue.push_back(completion(
            900,
            6002,
            size_of::<structs::SteamTimelineEventRecordingExists>(),
        ));
    });
    steam.pump();
    let CallState::Ready(answer) = steam.take(event_call) else {
        panic!("answered");
    };
    assert_eq!(
        answer,
        EventRecording {
            event: TimelineEventId(5),
            exists: true,
        }
    );

    fake(|f| f.call = 901);
    let phase_call = steam.timeline().phase_recording("run-12").unwrap();
    let mut phase = [0_u8; 64].to_vec();
    phase[..6].copy_from_slice(b"run-12");
    phase.extend_from_slice(&90_000_u64.to_le_bytes());
    phase.extend_from_slice(&15_000_u64.to_le_bytes());
    phase.extend_from_slice(&3_u32.to_le_bytes());
    phase.extend_from_slice(&4_u32.to_le_bytes());
    testing::script(|s| {
        s.results.push((901, phase, false));
        s.queue.push_back(completion(901, 6001, 88));
    });
    steam.pump();
    let CallState::Ready(answer) = steam.take(phase_call) else {
        panic!("answered");
    };
    assert_eq!(
        answer,
        PhaseRecording {
            phase: "run-12".to_owned(),
            recorded: Duration::from_secs(90),
            longest_clip: Duration::from_secs(15),
            clips: 3,
            screenshots: 4,
        }
    );
    assert_eq!(log(), ["event recording|5", "phase recording|run-12"]);
}

#[test]
fn a_recording_question_steam_does_not_start_is_refused() {
    let mut steam = steam();
    assert!(matches!(
        steam.timeline().event_recording(TimelineEventId(1)),
        Err(SteamError::Refused("DoesEventRecordingExist"))
    ));
    assert!(matches!(
        steam.timeline().phase_recording("p"),
        Err(SteamError::Refused("DoesGamePhaseRecordingExist"))
    ));
}
