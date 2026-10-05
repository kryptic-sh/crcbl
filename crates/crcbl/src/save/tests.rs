//! The save path's engine half: the slot rule, the typed line, the desk's
//! record and the storage section drawn from it.

use super::*;
use crcbl_ui::{DebugModule, DebugSection};

/// A save of `file` at `tick`, `bytes` long.
fn saved(file: &str, tick: u64, bytes: usize) -> Saved {
    Saved {
        file: file.to_owned(),
        tick: TickId::from_raw(tick),
        playtime_secs: 62.5,
        bytes,
        summary: "wave 3/10".to_owned(),
    }
}

/// The storage section `desk` draws, as `label: value` lines.
fn section_of(desk: &SaveDesk) -> Vec<String> {
    let mut section = DebugSection::default();
    desk.debug_section(&mut section);
    assert_eq!(section.title(), "storage");
    section
        .rows()
        .iter()
        .map(|row| format!("{}: {}", row.label, row.value))
        .collect()
}

/// **A slot is a bare name**: letters, digits, `-` and `_` pass, and
/// anything a path is made of, an empty name and an over-long one are each
/// refused with the rule in the line.
#[test]
fn a_slot_is_a_bare_name_and_never_a_path() {
    for name in ["slot2", "before-boss", "A_1"] {
        assert_eq!(
            Slot::new(name).map(|slot| slot.as_str().to_owned()),
            Ok(name.to_owned())
        );
    }
    let too_long = "s".repeat(SLOT_NAME_MAX + 1);
    for name in [
        "",
        "../run",
        "a/b",
        "a\\b",
        "C:run",
        "run.crb",
        too_long.as_str(),
    ] {
        let refused = Slot::new(name).expect_err(name);
        assert!(refused.contains("not a slot name"), "{name:?}: {refused}");
    }
    assert!(
        Slot::new(&"s".repeat(SLOT_NAME_MAX)).is_ok(),
        "the limit itself is a slot"
    );
}

/// **A slot's file sits beside the game's own**, its name before the
/// extension.
#[test]
fn a_slots_file_is_named_beside_the_games_own() {
    let slot = Slot::new("slot2").expect("a bare name");
    assert_eq!(slot.file_name("towers-run.crb"), "towers-run-slot2.crb");
    assert_eq!(slot.file_name("character"), "character-slot2");
    assert_eq!(
        SaveRequest::new(SaveTrigger::Console).file_name("towers-run.crb"),
        "towers-run.crb",
        "no slot is the game's own file"
    );
    assert_eq!(
        SaveRequest::new(SaveTrigger::Console)
            .in_slot(slot)
            .file_name("towers-run.crb"),
        "towers-run-slot2.crb"
    );
}

/// **A typed `save` takes nothing or one slot**, and the trigger is the one
/// it was read for.
#[test]
fn a_typed_save_takes_nothing_or_one_slot() {
    let own = SaveRequest::from_args(SaveTrigger::ServerConsole, &[]).expect("a bare save");
    assert_eq!(own.trigger(), SaveTrigger::ServerConsole);
    assert_eq!(own.slot(), None);
    let named = SaveRequest::from_args(SaveTrigger::Console, &["slot2"]).expect("one slot");
    assert_eq!(named.slot().map(Slot::as_str), Some("slot2"));
    assert!(SaveRequest::from_args(SaveTrigger::Console, &["a", "b"]).is_err());
    assert!(SaveRequest::from_args(SaveTrigger::Console, &["../a"]).is_err());
}

/// **The desk runs the writer once and records what it answered**: the last
/// save and its trigger, each file at its latest size, where the autosave
/// went, and a failure counted with its reason — which the writer's answer
/// reaches the trigger unchanged.
#[test]
fn the_desk_records_every_save_it_runs() {
    let mut desk = SaveDesk::new("data dir");
    let mut calls = Vec::new();
    let mut take = |desk: &mut SaveDesk, request: SaveRequest, outcome| {
        desk.take(request, |request| {
            calls.push(request.clone());
            outcome
        })
    };

    let first = take(
        &mut desk,
        SaveRequest::new(SaveTrigger::Autosave),
        Ok(saved("run.crb", 100, 300)),
    );
    assert_eq!(first, Ok(saved("run.crb", 100, 300)));
    let named = SaveRequest::new(SaveTrigger::Console).in_slot(Slot::new("b").expect("bare"));
    take(&mut desk, named.clone(), Ok(saved("run-b.crb", 140, 310))).expect("the writer's save");
    take(
        &mut desk,
        SaveRequest::new(SaveTrigger::Input),
        Ok(saved("run.crb", 180, 320)),
    )
    .expect("the writer's save");
    let refused = take(
        &mut desk,
        SaveRequest::new(SaveTrigger::Input),
        Err(SaveFailure::Refused("A WAVE IS COMING IN".to_owned())),
    );
    assert_eq!(
        refused,
        Err(SaveFailure::Refused("A WAVE IS COMING IN".to_owned()))
    );

    assert_eq!(calls.len(), 4, "the writer ran once a request");
    assert_eq!(
        calls[1], named,
        "the writer was handed the request as asked"
    );
    assert_eq!(
        desk.last().map(|last| last.tick),
        Some(TickId::from_raw(180))
    );
    assert_eq!(desk.last_trigger(), Some(SaveTrigger::Input));
    assert_eq!(
        section_of(&desk),
        [
            "where: data dir",
            "last save: tick 180, 62.5 s played",
            "by: key",
            "run.crb: 320 B",
            "run-b.crb: 310 B",
            "autosave: run.crb",
            "not saved: 1, last: A WAVE IS COMING IN",
        ]
    );
}

/// **The storage section shows the last save's tick**, and before any save
/// says there has been none rather than showing a tick that was never
/// written.
#[test]
fn the_storage_section_shows_the_last_saves_tick() {
    let mut desk = SaveDesk::new("nowhere");
    assert!(
        section_of(&desk).contains(&"last save: none this run".to_owned()),
        "{:?}",
        section_of(&desk)
    );
    desk.take(SaveRequest::new(SaveTrigger::Console), |_| {
        Ok(saved("run.crb", 3750, 64))
    })
    .expect("the writer's save");
    let rows = section_of(&desk);
    assert!(
        rows.contains(&"last save: tick 3750, 62.5 s played".to_owned()),
        "{rows:?}"
    );
}

/// **The console's line names the file and the tick, or why not.**
#[test]
fn the_console_line_names_the_save_or_why_not() {
    assert_eq!(
        console_line(&Ok(saved("run-b.crb", 42, 1))),
        "saved run-b.crb at tick 42: wave 3/10"
    );
    assert_eq!(
        console_line(&Err(SaveFailure::Failed("the disk is full".to_owned()))),
        "not saved: the disk is full"
    );
    assert_eq!(
        console_line(&Err(SaveFailure::Nowhere)),
        "not saved: this run keeps its saves nowhere"
    );
}
