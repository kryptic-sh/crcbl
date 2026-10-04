//! A drag sent over the protocol as one gesture: one entry of the server's
//! history and of every copy's, so one undo walks it all back on both — and
//! what seals a gesture before its last frame: another client's edit, an
//! undo in between, a lost link, a fetch, a save.
//!
//! Every frame moves block 0 along x ([`shift`]). Each test compares the
//! copy's saved text with the server's, and its history's length with the
//! server's: two histories that folded differently hold the same scene until
//! an undo, so the length is what shows a fold that went another way before
//! an undo makes it show in the text.

use crate::net::{EditGesture, EditOutcome, SessionState};

use super::*;

/// [`Rig::author`] in [`send`]; [`Rig::others`] are numbered from 1.
const AUTHOR: usize = 0;

/// Frame `id`'s gesture: carried on past, or its last.
const fn frame(id: u32, last: bool) -> Option<EditGesture> {
    Some(EditGesture { id, last })
}

/// Sends `op` from editor `who`, in `gesture` when it names one, and steps
/// until its reply comes.
fn send(rig: &mut Rig, who: usize, op: &EditOp, gesture: Option<EditGesture>) -> EditOutcome {
    let bytes = encode_op(op).expect("every op here travels");
    let client = editor(rig, who);
    let id = match gesture {
        Some(gesture) => client.send_edit_in(bytes, gesture),
        None => client.send_edit(bytes),
    }
    .expect("in session");
    for _ in 0..PATIENCE {
        rig.step();
        let replies: Vec<_> = editor(rig, who).edit_replies().collect();
        if let Some(reply) = replies.into_iter().find(|reply| reply.request_id == id) {
            return reply.outcome;
        }
    }
    panic!("no reply to request {id}");
}

fn editor(rig: &mut Rig, who: usize) -> &mut Client<InMemoryTransport> {
    match who {
        AUTHOR => &mut rig.author,
        other => &mut rig.others[other - 1],
    }
}

impl Rig {
    /// Adds a client that edits, steps until it is in session, and returns
    /// its number for [`send`].
    fn add_editor(&mut self) -> usize {
        let client = client_of(&mut self.server);
        self.others.push(client);
        let who = self.others.len();
        self.until(|rig| rig.others[who - 1].session_id().is_some());
        who
    }

    /// The server's history's length, and the copy's.
    fn history_lengths(&self) -> (usize, usize) {
        let copy = self.follower().document().expect("a copy").log().len();
        (self.server.document().log().len(), copy)
    }

    /// Steps until the copy is current, and holds it to the server's scene
    /// and the server's history's length, which it returns.
    fn in_step(&mut self, what: &str) -> usize {
        self.caught_up();
        let (served, copy) = self.both_files();
        assert_eq!(copy, served, "the copy is not the server's scene {what}");
        let (served, copy) = self.history_lengths();
        assert_eq!(copy, served, "the copy's history folded otherwise {what}");
        served
    }
}

fn applied(outcome: &EditOutcome) {
    assert!(
        matches!(outcome, EditOutcome::Applied { .. }),
        "{outcome:?}"
    );
}

/// A server with a follower caught up on [`one_block`], and the scene's text
/// before any edit.
fn following() -> (Rig, BTreeMap<String, String>) {
    let mut rig = Rig::new(one_block());
    rig.join();
    rig.caught_up();
    let before = rig.both_files().0;
    (rig, before)
}

/// **A five-frame drag sent as one gesture is one entry**, on the server and
/// on the copy at every frame; one undo over the protocol puts the scene back
/// exactly as it was before the drag, on both — and the copy took that undo
/// itself, from its own history, with no second fetch.
#[test]
fn a_five_frame_drag_is_one_entry_and_one_undo_on_the_server_and_the_copy() {
    let (mut rig, before) = following();
    for (index, x) in [1.0, 2.0, 3.0, 4.0, 5.0].into_iter().enumerate() {
        let last = index == 4;
        applied(&send(&mut rig, AUTHOR, &shift(x), frame(7, last)));
        assert_eq!(rig.in_step(&format!("after frame {index}")), 1);
        assert_eq!(rig.server.gesture_open(), !last);
    }

    applied(&send(&mut rig, AUTHOR, &EditOp::Undo, None));
    rig.in_step("after the undo");
    let (served, copy) = rig.both_files();
    assert_eq!(served, before, "the undo did not reach the drag's start");
    assert_eq!(copy, before);
    assert_eq!(rig.follower().fetch_count(), 1, "the copy refetched");
}

/// **The last frame ends the gesture**: a frame naming the same id after it
/// is an entry of its own, so the undo takes back only that frame.
#[test]
fn the_last_frame_ends_the_gesture() {
    let (mut rig, _) = following();
    applied(&send(&mut rig, AUTHOR, &shift(1.0), frame(3, false)));
    applied(&send(&mut rig, AUTHOR, &shift(2.0), frame(3, true)));
    applied(&send(&mut rig, AUTHOR, &shift(3.0), frame(3, false)));
    assert_eq!(rig.in_step("after the frames"), 2);

    applied(&send(&mut rig, AUTHOR, &EditOp::Undo, None));
    rig.in_step("after the undo");
    let mut at_the_last_frame = one_block();
    let EditOp::Apply(command) = shift(2.0) else {
        unreachable!("a shift is a command");
    };
    at_the_last_frame.apply(command).expect("block 0 moves");
    assert_eq!(
        rig.both_files().0,
        at_the_last_frame.files().expect("saves"),
        "the undo did not stop at the gesture's last frame"
    );
}

/// **An edit in no gesture is an entry of its own**, each one.
#[test]
fn an_edit_in_no_gesture_is_one_entry_each() {
    let (mut rig, _) = following();
    for x in [1.0, 2.0, 3.0] {
        applied(&send(&mut rig, AUTHOR, &shift(x), None));
    }
    assert_eq!(rig.in_step("after three edits"), 3);
}

/// **Two clients' drags interleaving are an entry per frame**, decided
/// 2026-10-05: another client's frame seals the gesture on top, since only
/// the entry on top can fold. Both number their gesture 1 — a client's id is
/// compared only with its own — and four undos walk all of it back.
#[test]
fn two_clients_drags_interleaving_seal_each_other() {
    let (mut rig, before) = following();
    let other = rig.add_editor();
    for (who, x) in [(AUTHOR, 1.0), (other, 2.0), (AUTHOR, 3.0), (other, 4.0)] {
        applied(&send(&mut rig, who, &shift(x), frame(1, false)));
    }
    assert_eq!(rig.in_step("after the interleaved frames"), 4);
    for _ in 0..4 {
        applied(&send(&mut rig, other, &EditOp::Undo, None));
    }
    rig.in_step("after the undos");
    assert_eq!(rig.both_files().0, before);
}

/// **Another client's undo and redo in the middle of a drag seal it**: the
/// redo puts the drag's entry back on top, and the drag's next frame is an
/// entry of its own all the same — anything applied in between ends a
/// gesture, whoever applied it.
#[test]
fn another_clients_undo_and_redo_seal_the_drag_under_them() {
    let (mut rig, _) = following();
    let other = rig.add_editor();
    applied(&send(&mut rig, AUTHOR, &shift(1.0), frame(2, false)));
    applied(&send(&mut rig, AUTHOR, &shift(2.0), frame(2, false)));
    assert_eq!(rig.in_step("after the drag's frames"), 1);
    applied(&send(&mut rig, other, &EditOp::Undo, None));
    applied(&send(&mut rig, other, &EditOp::Redo, None));
    applied(&send(&mut rig, AUTHOR, &shift(3.0), frame(2, false)));
    assert_eq!(rig.in_step("after the drag carried on"), 2);
}

/// **A client's link going down mid-drag seals its gesture**: the server
/// holds no gesture open once it has seen the link go.
#[test]
fn a_lost_link_mid_drag_seals_the_gesture() {
    let (mut rig, _) = following();
    let other = rig.add_editor();
    applied(&send(&mut rig, other, &shift(1.0), frame(5, false)));
    applied(&send(&mut rig, other, &shift(2.0), frame(5, false)));
    assert!(rig.server.gesture_open());

    drop(rig.others.remove(other - 1));
    let lost = rig.server.host().peers().last().expect("the second editor");
    rig.until(|rig| rig.server.host().peer_state(lost) != Some(SessionState::Connected));
    rig.step();
    assert!(!rig.server.gesture_open(), "the lost client's drag is open");
    assert_eq!(rig.in_step("after the link went"), 1);
}

/// **Answering a fetch seals the gesture on top**: a copy joining mid-drag
/// holds none of the drag's entry so far, so the drag carries on as an entry
/// of its own on both, and the undo of it lands the copy where it lands the
/// server.
#[test]
fn a_fetch_mid_drag_seals_the_gesture() {
    let mut rig = Rig::new(one_block());
    applied(&send(&mut rig, AUTHOR, &shift(1.0), frame(4, false)));
    applied(&send(&mut rig, AUTHOR, &shift(2.0), frame(4, false)));
    rig.join();
    rig.caught_up();
    applied(&send(&mut rig, AUTHOR, &shift(3.0), frame(4, true)));
    rig.caught_up();
    assert_eq!(rig.history_lengths(), (2, 1));

    applied(&send(&mut rig, AUTHOR, &EditOp::Undo, None));
    rig.caught_up();
    let (served, copy) = rig.both_files();
    assert_eq!(copy, served, "the copy's undo landed elsewhere");
    assert_eq!(rig.follower().fetch_count(), 1);
}

/// **A save mid-drag seals the gesture on the server and the copy alike**:
/// the save closes the entry on top, so the drag's next frame is an entry
/// of its own — and the notice says so, so the copy pushes where the server
/// does rather than fold.
#[test]
fn a_save_mid_drag_splits_the_drag_on_the_server_and_the_copy_alike() {
    let (mut rig, _) = following();
    applied(&send(&mut rig, AUTHOR, &shift(1.0), frame(6, false)));
    let saved = tempfile::tempdir().expect("a temporary directory");
    rig.server
        .document_mut()
        .save_to(saved.path().join("one.scn"))
        .expect("the scene saves");
    assert!(!rig.server.gesture_open(), "a save left the drag open");
    applied(&send(&mut rig, AUTHOR, &shift(2.0), frame(6, false)));
    assert_eq!(rig.in_step("after the drag carried on"), 2);

    applied(&send(&mut rig, AUTHOR, &EditOp::Undo, None));
    rig.in_step("after the undo");
}

/// **An undo or a redo in a gesture is refused as malformed**, and changes
/// nothing.
#[test]
fn a_step_of_the_history_in_a_gesture_is_refused() {
    let (mut rig, _) = following();
    applied(&send(&mut rig, AUTHOR, &shift(1.0), None));
    for op in [EditOp::Undo, EditOp::Redo] {
        let EditOutcome::Refused { reason, .. } = send(&mut rig, AUTHOR, &op, frame(1, false))
        else {
            panic!("{op:?} in a gesture applied");
        };
        assert_eq!(reason, crate::net::EditRefusal::MALFORMED);
    }
    assert_eq!(rig.server.revision(), 1);
    assert_eq!(rig.in_step("after the refusals"), 1);
}

/// **A notice naming a step of the history part of a gesture makes the copy
/// stale** rather than step: no server sends one, so the copy cannot know
/// what was meant, and fetches.
#[test]
fn a_notice_of_a_step_in_a_gesture_makes_the_copy_stale() {
    let mut follower = following_at(3);
    follower.take(notice(4, &shift(1.0)));
    let mut undo = notice(5, &EditOp::Undo);
    undo.gesture = Some(1);
    follower.take(undo);
    assert!(follower.is_stale(), "the copy stepped its history");
    assert_eq!(follower.revision(), Some(4));
}
