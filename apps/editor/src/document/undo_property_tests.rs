//! **`docs/plan/08-editor.md`'s second exit criterion: undo and redo correct
//! across every edit command**, as a property test — random sequences of every
//! edit the editor makes, interleaved with undos and redos, each step's state
//! checked against the state the log says it stands at.
//!
//! Each step is played through the entry point the UI calls for it
//! (`play::play`), so a step the document refuses is refused here too and
//! counted, not skipped: a history that only ever had its edits refused would
//! otherwise pass by walking nothing. [`walk`] holds a model of the history
//! beside the document — every state the log has stood at, where it stands,
//! and which of those states the last save was taken at while the log still
//! holds it — and checks after each step:
//!
//! - a refused step changed nothing and recorded nothing;
//! - an accepted step recorded exactly one entry, dropping any redo above it —
//!   a drag included, whatever leaves its frames named, or none for a drag
//!   that ended where it began, whose state must then be the one before it
//!   and whose redo above it must still be there, as deep and as exact;
//! - an undo or a redo landed on exactly the state recorded at the position
//!   it moved to;
//! - the selection names only entities the document holds;
//! - the document is clean exactly when the log stands on the state the last
//!   save was taken at — the one it opened in until the first save — which an
//!   edit dropping that state's entry makes impossible until the next save,
//!   and a drag back to its start does not; and a clean document holds
//!   exactly the state that save wrote.
//!
//! That last check is not "dirty exactly when the state differs from the
//! saved one": an edit and a second one putting it back are two entries the
//! file never stood on, and read dirty though the state is the saved one —
//! the decision of 2026-10-03, `docs/plan/08-editor.md`. Clean implies the
//! saved state, which is what a position-only marker breaks.
//!
//! After the last step the whole log is undone, each step down checked, to
//! the state it opened in; then redone to the top, each step up checked. What
//! is compared is `state::State` — the saved files, every entity's rows in
//! every registered system and its collider — which says what it covers.
//!
//! **Not covered**: play mode (every edit is refused in it, which
//! `play_tests` holds), a save into the document's own directory (each save
//! here is a copy into a directory of its own, which marks the log the same
//! way), and a mesh's measured box beyond the collider it builds.

mod ops;
mod play;
mod state;

use std::cell::RefCell;
use std::collections::BTreeMap;

use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestCaseResult, TestRunner};

use super::Document;
use super::mesh_tests::assets;
use super::systems_tests::two_systems;
use crate::command::EditCommand;

use ops::{EVERY_OP, Op};
use play::{Outcome, Reached};
use state::State;

/// How many histories a run plays, each a fresh document.
const CASES: u32 = 512;

/// The longest history generated, undos and redos included.
const MAX_STEPS: usize = 40;

/// The least share of the edit steps played — undos and redos left out — the
/// document must accept, in percent, so a run whose generators had drifted
/// into producing only refusals fails rather than passing on an empty log.
const LEAST_ACCEPTED_PERCENT: usize = 40;

/// What a run counts, across every history it plays.
#[derive(Default)]
struct Tally {
    /// Each step's outcomes, by [`Op::name`].
    outcomes: BTreeMap<&'static str, BTreeMap<Outcome, usize>>,
    /// Each [`EditCommand`] variant a recorded entry held, at any depth of a
    /// batch, by [`variant`].
    commands: BTreeMap<&'static str, usize>,
    /// Facts some history must reach, by what `play::play` reports and
    /// [`walk`] notes.
    reached: BTreeMap<&'static str, usize>,
}

/// The facts [`Tally::reached`] must hold — each a shape of edit whose undo
/// has its own way to go wrong.
const MUST_REACH: [&str; 16] = [
    "a gesture of several writes",
    "a drag whose leaves change part-way",
    "a gesture that ended where it began",
    "a gesture back to its start under redo",
    "a nudge of two entities",
    "a switch to another variant",
    "a drag of two entities",
    "a delete of two entities",
    "a duplicate of two entities",
    "a drop listing meshes",
    "an attach listing its system",
    "an unlisting from the manifest's middle",
    "a state holding a name",
    "an edit dropping the redo above it",
    "a refusal with redo above it",
    "an edit dropping the saved state",
];

/// **The exit criterion.** See the module docs for what each history checks;
/// this plays [`CASES`] of them and then checks the run itself reached every
/// command and every shape of edit, so it cannot pass by doing nothing.
#[test]
fn random_histories_walk_back_through_every_state() {
    let tally = RefCell::new(Tally::default());
    let mut runner = TestRunner::new(ProptestConfig {
        cases: CASES,
        source_file: Some(file!()),
        test_name: Some(concat!(
            module_path!(),
            "::random_histories_walk_back_through_every_state"
        )),
        ..ProptestConfig::default()
    });
    let histories = proptest::collection::vec(ops::op(), 1..=MAX_STEPS);
    if let Err(error) = runner.run(&histories, |steps| walk(&steps, &tally)) {
        panic!("{error}\n{runner}");
    }
    tally.into_inner().assert_the_run_reached_everything();
}

/// Plays `steps` on a fresh [`two_systems`] document, checking each against
/// the model of the history, then walks the whole log down and back up.
fn walk(steps: &[Op], tally: &RefCell<Tally>) -> TestCaseResult {
    let mut document = two_systems();
    document.set_assets(Box::new(assets()));
    let opened = State::of(&mut document);
    let mut history = History {
        states: vec![opened.clone()],
        position: 0,
        saved: Some(0),
        saved_state: opened,
    };
    for (index, op) in steps.iter().enumerate() {
        let top = history.states.len() - 1;
        let mut reached = Reached::new();
        let mut outcome = play::play(&mut document, op, &mut reached);
        // A gesture's leaves that end where they began drop out of its entry,
        // and an entry left with none goes (`UndoLog::record_in`), so a drag
        // back to its start is accepted and records nothing — which `check`
        // then holds to the state it started from. The redo above the log
        // stays: its first write held it aside and the entry's going put it
        // back, so the model keeps every state above, which `check` holds to
        // the log's length and the walk up at the end to each state.
        let gesture = matches!(op, Op::Drag { .. } | Op::FieldDrag { .. });
        if gesture && outcome == Outcome::Recorded && document.log().position() == history.position
        {
            outcome = Outcome::Unchanged;
            reached.push("a gesture that ended where it began");
            if history.position < top {
                reached.push("a gesture back to its start under redo");
            }
            prop_assert_eq!(
                document.log().len() - document.log().position(),
                top - history.position,
                "step {}: a drag back to its start changed the redo's depth",
                index
            );
        }
        let mut tally = tally.borrow_mut();
        *tally
            .outcomes
            .entry(op.name())
            .or_default()
            .entry(outcome)
            .or_default() += 1;
        for fact in reached {
            tally.note(fact);
        }
        let at = format!("step {index}, {op:?} ({outcome:?})");
        match outcome {
            Outcome::Recorded => {
                if history.position < top {
                    tally.note("an edit dropping the redo above it");
                }
                if history.saved.is_some_and(|at| at > history.position) {
                    tally.note("an edit dropping the saved state");
                    history.saved = None;
                }
                history.states.truncate(history.position + 1);
                let now = State::of(&mut document);
                if now.has_names() {
                    tally.note("a state holding a name");
                }
                history.states.push(now);
                history.position += 1;
                let entry = document.log().applied().last();
                prop_assert!(entry.is_some(), "{at}: nothing applied");
                count_commands(entry.into_iter(), &mut tally.commands);
            }
            Outcome::Walked if matches!(op, Op::Undo) => {
                prop_assert!(history.position > 0, "{at}: an undo below the log");
                history.position -= 1;
            }
            Outcome::Walked => {
                prop_assert!(history.position < top, "{at}: a redo above the log");
                history.position += 1;
            }
            Outcome::AtEnd => {
                let end = if matches!(op, Op::Undo) { 0 } else { top };
                prop_assert_eq!(history.position, end, "{}: the log stopped early", at);
            }
            Outcome::Refused => {
                if history.position < top {
                    tally.note("a refusal with redo above it");
                }
            }
            Outcome::Saved => {
                history.saved = Some(history.position);
                history.saved_state = State::of(&mut document);
            }
            Outcome::Unchanged | Outcome::Skipped => {}
        }
        drop(tally);
        history.check(&mut document, &at)?;
    }

    while history.position > 0 {
        prop_assert!(document.undo().expect("an entry's inverse applies"));
        history.position -= 1;
        history.check(&mut document, "undoing the whole log")?;
    }
    prop_assert!(!document.undo().expect("nothing left"), "undo past the log");
    let top = history.states.len() - 1;
    while history.position < top {
        prop_assert!(document.redo().expect("an entry applies again"));
        history.position += 1;
        history.check(&mut document, "redoing the whole log")?;
    }
    prop_assert!(
        !document.redo().expect("nothing above"),
        "redo past the log"
    );
    Ok(())
}

/// The model of a document's history: the state at every position the log
/// holds — the state it opened in first — the position it stands at, and the
/// position of the state the last save was taken at.
struct History {
    states: Vec<State>,
    position: usize,
    /// Where in [`states`](Self::states) the last save was taken — `0`, the
    /// state the document opened in, before any — or [`None`] once an edit
    /// has dropped that state's entry.
    saved: Option<usize>,
    /// What the last save wrote, read when it was taken rather than from
    /// [`states`](Self::states), so a clean document is held to it whatever
    /// the model says.
    saved_state: State,
}

impl History {
    /// Fails, saying where (`at`), unless `document` stands where the model
    /// says and holds the state recorded there.
    fn check(&self, document: &mut Document, at: &str) -> TestCaseResult {
        prop_assert_eq!(
            document.log().position(),
            self.position,
            "{}: the log's position",
            at
        );
        prop_assert_eq!(
            document.log().len(),
            self.states.len() - 1,
            "{}: the log's length",
            at
        );
        prop_assert_eq!(
            document.is_dirty(),
            self.saved != Some(self.position),
            "{}: the dirty marker, the last save at {:?}",
            at,
            self.saved
        );
        let selection = document.selection().to_vec();
        for (index, id) in selection.iter().enumerate() {
            prop_assert!(
                document.ids.entity(*id).is_some() && !selection[..index].contains(id),
                "{}: the selection {:?} names #{} twice or not at all",
                at,
                selection,
                id
            );
        }
        let now = State::of(document);
        if !document.is_dirty() && now != self.saved_state {
            return Err(TestCaseError::fail(format!(
                "{at}: clean, and the state is not the one last saved:\n{}",
                now.difference(&self.saved_state),
            )));
        }
        let expected = &self.states[self.position];
        if now != *expected {
            return Err(TestCaseError::fail(format!(
                "{at}: the state is not the one recorded at position {}:\n{}",
                self.position,
                now.difference(expected),
            )));
        }
        Ok(())
    }
}

/// Counts every command `commands` holds, each batch's members too, by
/// [`variant`].
fn count_commands<'a>(
    commands: impl Iterator<Item = &'a EditCommand>,
    into: &mut BTreeMap<&'static str, usize>,
) {
    for command in commands {
        *into.entry(variant(command)).or_default() += 1;
        if let EditCommand::Batch(members) = command {
            count_commands(members.iter(), into);
        }
    }
}

/// `command`'s variant, by name.
///
/// **A match with no wildcard**, so a variant added to [`EditCommand`] does not
/// compile here until it is named — and naming it means adding it to
/// [`EVERY_COMMAND`], which fails the run until some generated step records it.
const fn variant(command: &EditCommand) -> &'static str {
    match command {
        EditCommand::SetProperty { .. } => "SetProperty",
        EditCommand::SetVariant { .. } => "SetVariant",
        EditCommand::Spawn { .. } => "Spawn",
        EditCommand::Delete { .. } => "Delete",
        EditCommand::Attach { .. } => "Attach",
        EditCommand::Detach { .. } => "Detach",
        EditCommand::ListSystem { .. } => "ListSystem",
        EditCommand::UnlistSystem { .. } => "UnlistSystem",
        EditCommand::Rename { .. } => "Rename",
        EditCommand::Batch(_) => "Batch",
    }
}

/// Every name [`variant`] answers.
const EVERY_COMMAND: [&str; 10] = [
    "SetProperty",
    "SetVariant",
    "Spawn",
    "Delete",
    "Attach",
    "Detach",
    "ListSystem",
    "UnlistSystem",
    "Rename",
    "Batch",
];

impl Tally {
    /// Counts one more history reaching `fact`.
    fn note(&mut self, fact: &'static str) {
        *self.reached.entry(fact).or_default() += 1;
    }

    /// Panics naming the first command, step or shape of edit no history
    /// reached, or if the document accepted too few of the edits played.
    fn assert_the_run_reached_everything(&self) {
        for command in EVERY_COMMAND {
            assert!(
                self.commands.get(command).copied().unwrap_or(0) > 0,
                "no history recorded an EditCommand::{command}: {:?}",
                self.commands,
            );
        }
        let count = |name: &str, outcome: Outcome| {
            self.outcomes
                .get(name)
                .and_then(|outcomes| outcomes.get(&outcome))
                .copied()
                .unwrap_or(0)
        };
        for name in EVERY_OP {
            let moved = count(name, Outcome::Recorded)
                + count(name, Outcome::Walked)
                + count(name, Outcome::Saved);
            assert!(moved > 0, "no history had a {name} accepted");
        }
        for fact in MUST_REACH {
            assert!(
                self.reached.get(fact).copied().unwrap_or(0) > 0,
                "no history reached {fact}",
            );
        }
        let (mut accepted, mut refused, mut played) = (0, 0, 0);
        for (name, outcomes) in &self.outcomes {
            if matches!(*name, "undo" | "redo" | "save") {
                continue;
            }
            for (outcome, count) in outcomes {
                match outcome {
                    Outcome::Recorded | Outcome::Unchanged => accepted += count,
                    Outcome::Refused => refused += count,
                    Outcome::Skipped | Outcome::Walked | Outcome::AtEnd | Outcome::Saved => {}
                }
                played += count;
            }
        }
        assert!(refused > 0, "no edit was refused: {:?}", self.outcomes);
        assert!(
            accepted * 100 >= played * LEAST_ACCEPTED_PERCENT,
            "the document accepted {accepted} of {played} edits: {:?}",
            self.outcomes,
        );
    }
}
