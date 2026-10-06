use super::*;
use crate::input::{ActionDecl, ActionKind};
use crate::store::record::Backing;

/// Replacing on a device keeps every other device's binding where it was.
#[test]
fn a_capture_replaces_only_its_own_device() {
    let bindings = [
        Binding::Key(KeyCode::Space),
        Binding::PadButton(PadButton::South),
        Binding::Key(KeyCode::KeyK),
    ];
    assert_eq!(
        replaced_on_device(&bindings, &Binding::Key(KeyCode::KeyJ)),
        [
            Binding::Key(KeyCode::KeyJ),
            Binding::PadButton(PadButton::South),
        ]
    );
    assert_eq!(
        replaced_on_device(&bindings, &Binding::MouseButton(PointerButton::Right)),
        [
            Binding::Key(KeyCode::Space),
            Binding::PadButton(PadButton::South),
            Binding::Key(KeyCode::KeyK),
            Binding::MouseButton(PointerButton::Right),
        ]
    );
}

/// Every row id names its row back, and no id in the block is shared.
#[test]
fn every_action_row_names_its_row_back() {
    let ids = RebindIds::starting_at(40);
    let rows = 3;
    for index in 0..rows {
        assert_eq!(ids.action_of(ids.action(index), rows), Some(index));
    }
    assert_eq!(ids.action_of(ids.cancel(), rows), None);
    assert_eq!(ids.action_of(ids.action(rows), rows), None);
    let mut all: Vec<WidgetId> = (0..rows).map(|index| ids.action(index)).collect();
    all.extend([ids.reset(), ids.back(), ids.swap(), ids.cancel()]);
    let count = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), count, "two rows share an id");
}

/// A flow over `jump` on Space, `run` on both Shifts, and `look` on Q — an
/// action the page does not list — kept nowhere.
fn flow() -> Rebinder {
    let mut map = ActionMap::new();
    for (name, keys) in [
        ("jump", vec![KeyCode::Space]),
        ("run", vec![KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        ("look", vec![KeyCode::KeyQ]),
    ] {
        map.declare(ActionDecl {
            name: name.to_owned(),
            kind: ActionKind::Button,
            bindings: keys.into_iter().map(Binding::Key).collect(),
        });
    }
    Rebinder::open(
        map,
        vec![
            RebindRow {
                name: "jump",
                label: "JUMP",
            },
            RebindRow {
                name: "run",
                label: "RUN",
            },
        ],
        ProfileStore::open(Backing::None, crate::store::profile::PROFILE_FILE),
    )
}

/// **Two bindings that print the same are one entry on the row**: the two
/// Shifts read `Shift`, not `Shift / Shift`.
#[test]
fn a_row_names_each_label_once() {
    let flow = flow();
    assert_eq!(flow.hint(1), "Shift");
    assert_eq!(flow.hint(0), "Space");
}

/// **A clash with an action the page does not list still asks**, names that
/// action by its name, and `SWAP` gives it the old input — the player saw
/// what moved.
#[test]
fn a_clash_with_an_unlisted_action_asks_and_names_it() {
    let mut flow = flow();
    flow.listen(0);
    flow.key(KeyCode::KeyQ, true);
    assert_eq!(
        flow.capture(),
        &Capture::Conflict {
            action: 0,
            binding: Binding::Key(KeyCode::KeyQ),
            other: "look".to_owned(),
        },
        "Q was taken from an action the page does not list without asking",
    );
    let said: Vec<String> = flow
        .conflict_subtitle()
        .into_iter()
        .map(|line| line.text)
        .collect();
    assert!(
        said.iter().any(|line| line == "Q IS ON LOOK"),
        "the clash is not named: {said:?}"
    );

    flow.swap();
    assert_eq!(
        flow.actions().bindings("jump"),
        Some(&[Binding::Key(KeyCode::KeyQ)][..])
    );
    assert_eq!(
        flow.actions().bindings("look"),
        Some(&[Binding::Key(KeyCode::Space)][..])
    );
}
