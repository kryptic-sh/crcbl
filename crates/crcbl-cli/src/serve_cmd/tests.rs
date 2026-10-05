//! `crcbl edit --serve`'s server in process, on UDP loopback, against
//! temporary copies of towers' committed field: clients of the edit protocol
//! — [`Client`]s with a [`SceneFollower`] each — fetch it, edit it, undo over
//! the protocol and follow it, and the files the server saves are held to
//! what the editor's document writes for the same edits.
//!
//! As towers' serve tests do, the server listens on `127.0.0.1:0` and every
//! client sends to it there (a client's own socket is the one
//! [`UdpTransport::connect`] binds, any free port), the server is stepped a
//! frame at a time on a clock of the test's own, and the serve loop runs with
//! a console over a channel the test types into; loopback delivers
//! asynchronously, so each step is followed by a short pause, and every wait
//! is bounded by [`MAX_FRAMES`].

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc;

use crcbl::client::{Client, Ended};
use crcbl::ecs::World;
use crcbl::net::udp::UdpTransport;
use crcbl::net::{EditGesture, EditOutcome, EditRefusal, InMemoryTransport, Message, Transport};
use crcbl::reflect::Value;
use crcbl::scene::edit::{EditCommand, EditOp, encode_op};
use crcbl::scene::scn::SceneEntityId;
use crcbl::scene_edit::{HISTORY, SCENE_LOCK, SceneFollower, lock_scene};

use super::*;
use crate::report::EXIT_LOCKED;
use crate::scene_args::{EditArgs, SceneArgs, SceneEdit, SceneVerb};

/// One frame at [`EDIT_TICK_HZ`].
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / EDIT_TICK_HZ as u64);

/// The most frames any wait here runs: ten seconds of the server's time, and
/// a few seconds of wall time at [`PAUSE`] a step.
const MAX_FRAMES: usize = 600;

/// The pause after each step, for loopback to deliver.
const PAUSE: Duration = Duration::from_millis(1);

/// Plot 4, `entry`, in towers' field.
const ENTRY: SceneEntityId = SceneEntityId(4);

/// Loopback, any free port.
fn loopback() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

/// A private directory that cleans itself up, as the binary's tests have:
/// this crate takes no `tempfile` for a test helper.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "crcbl-cli-serve-{label}-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temporary directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A copy of towers' committed field under `temp`, named `name`.
fn field_copy(temp: &TempDir, name: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the CLI lives two levels below the workspace root")
        .join("apps/towers/assets/scenes/field.scn");
    let to = temp.0.join(name);
    copy_tree(&from, &to);
    to
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a fresh directory");
    for entry in std::fs::read_dir(from).expect("a readable directory") {
        let entry = entry.expect("a directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("a copied file");
        }
    }
}

/// The scene's files under `dir` — every file but its history and its lock
/// — as text, keyed as [`Document::files`] keys them.
fn scene_files(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    collect(dir, dir, &mut files);
    files
}

fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) {
    for entry in std::fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("a directory entry").path();
        let name = path.file_name().and_then(|name| name.to_str());
        if path.is_dir() {
            collect(root, &path, files);
        } else if name != Some(SCENE_LOCK) && name != Some(HISTORY) {
            let key = path
                .strip_prefix(root)
                .expect("under the root")
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&path).expect("a scene file is text");
            files.insert(key, text);
        }
    }
}

/// `entry` moved to `(x, 0, -2.25)`, as `crcbl scene move` writes it: one
/// entry of three writes to its plot's position.
fn move_entry(x: f64) -> EditCommand {
    let axis = |axis: usize, value: f64| EditCommand::SetProperty {
        entity: ENTRY,
        system: "plots".to_owned(),
        path: format!("position.{axis}"),
        value: Value::Float(value),
    };
    EditCommand::Batch(vec![axis(0, x), axis(1, 0.0), axis(2, -2.25)])
}

/// A client of the server and its copy of the scene, with the replies to
/// its edits gathered as they come.
struct Joined {
    client: Client<UdpTransport>,
    follower: SceneFollower,
    replies: Vec<crcbl::net::EditReply>,
}

/// A server, the clients joined to it, and the clock they share.
struct Rig {
    server: Server,
    clients: Vec<Joined>,
    now: Duration,
    printed: Vec<String>,
}

impl Rig {
    /// The scene at `dir`, served on loopback, with nobody joined.
    fn serving(dir: &Path) -> Self {
        Self {
            server: Server::open(dir, loopback())
                .expect("loopback UDP must be available to these tests"),
            clients: Vec::new(),
            now: Duration::ZERO,
            printed: Vec::new(),
        }
    }

    /// A client connecting now, following the scene; its index.
    fn join(&mut self) -> usize {
        let vocabulary = crcbl_editor::scene::vocabulary();
        let transport =
            UdpTransport::connect(self.server.local_addr(), EDIT_PROTOCOL_ID).expect("a socket");
        self.clients.push(Joined {
            client: Client::new_with_compatibility(
                World::new(),
                transport,
                EDIT_TICK_HZ,
                edit_compatibility(&vocabulary),
                next_player(),
            ),
            follower: SceneFollower::new(vocabulary),
            replies: Vec::new(),
        });
        self.clients.len() - 1
    }

    fn step(&mut self) {
        self.now += FRAME;
        if let Some(line) = self.server.frame(self.now) {
            self.printed.push(line);
        }
        for joined in &mut self.clients {
            joined.client.update(self.now);
            joined.follower.update(&mut joined.client, self.now);
            joined.replies.extend(joined.client.edit_replies());
        }
        thread::sleep(PAUSE);
    }

    /// Steps until `done` holds, or panics naming `what`.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        for _ in 0..MAX_FRAMES {
            self.step();
            if done(self) {
                return;
            }
        }
        panic!("never happened: {what}");
    }

    /// Steps until every client's copy stands at the server's revision.
    fn caught_up(&mut self) {
        self.until("every copy at the server's revision", |rig| {
            let revision = rig.server.edit.revision();
            rig.clients.iter().all(|joined| {
                joined.follower.revision() == Some(revision) && !joined.follower.is_stale()
            })
        });
    }

    /// Sends `op` from client `index`, and steps until its reply comes.
    fn send(&mut self, index: usize, op: &EditOp) -> EditOutcome {
        self.send_bytes(index, encode_op(op).expect("every op here travels"))
    }

    /// Sends `bytes` as an operation from client `index`, and steps until
    /// its reply comes.
    fn send_bytes(&mut self, index: usize, bytes: Vec<u8>) -> EditOutcome {
        self.send_request(index, bytes, None)
    }

    /// Sends `op` from client `index` as one edit of `gesture`, and steps
    /// until its reply comes.
    fn send_in(&mut self, index: usize, op: &EditOp, gesture: EditGesture) -> EditOutcome {
        let bytes = encode_op(op).expect("every op here travels");
        self.send_request(index, bytes, Some(gesture))
    }

    fn send_request(
        &mut self,
        index: usize,
        bytes: Vec<u8>,
        gesture: Option<EditGesture>,
    ) -> EditOutcome {
        self.until("the client in session", |rig| {
            rig.clients[index].client.session_id().is_some()
        });
        let client = &mut self.clients[index].client;
        let id = match gesture {
            Some(gesture) => client.send_edit_in(bytes, gesture),
            None => client.send_edit(bytes),
        }
        .expect("in session");
        self.until("the reply", |rig| {
            rig.clients[index]
                .replies
                .iter()
                .any(|reply| reply.request_id == id)
        });
        let replies = &mut self.clients[index].replies;
        let at = replies
            .iter()
            .position(|reply| reply.request_id == id)
            .expect("found above");
        replies.remove(at).outcome
    }

    /// Each client's copy of the scene, as saved text.
    fn copies(&mut self) -> Vec<BTreeMap<String, String>> {
        self.clients
            .iter_mut()
            .map(|joined| {
                joined
                    .follower
                    .document_mut()
                    .expect("a copy")
                    .files()
                    .expect("a copy saves")
            })
            .collect()
    }
}

/// **Clients fetch a served scene, edit it, undo over the protocol and
/// follow it, and every applied edit is saved as the document writes it.**
/// A client moves a plot and is answered `Applied`; the files on disk are
/// then what the editor's document writes for that move, before any `quit`.
/// A second client joins late and follows to the same bytes; its undo puts
/// the committed bytes back on disk and in both copies; an undo past the
/// history is refused. `quit` at the console tells both clients the server
/// shut down, lets the lock go, and leaves the history beside the scene, the
/// move undone and redoable.
#[test]
fn clients_edit_and_follow_a_served_scene_saved_after_each_edit() {
    let temp = TempDir::new("follow");
    let dir = field_copy(&temp, "field.scn");
    let expected_dir = field_copy(&temp, "expected.scn");
    let before = scene_files(&dir);
    let mut rig = Rig::serving(&dir);

    let author = rig.join();
    rig.caught_up();
    assert_eq!(
        rig.send(author, &EditOp::Apply(move_entry(1.5))),
        EditOutcome::Applied { revision: 1 }
    );
    let mut expected =
        Document::open_dir(&expected_dir, crcbl_editor::scene::vocabulary()).expect("the field");
    expected.apply(move_entry(1.5)).expect("a plot moves");
    expected.save().expect("the copy saves");
    let moved = scene_files(&expected_dir);
    assert_ne!(moved, before, "the move changed nothing");
    assert_eq!(scene_files(&dir), moved, "the applied move was not saved");

    let reader = rig.join();
    rig.caught_up();
    for copy in rig.copies() {
        assert_eq!(copy, moved, "a copy is not the served scene");
    }

    assert_eq!(
        rig.send(reader, &EditOp::Undo),
        EditOutcome::Applied { revision: 2 }
    );
    assert_eq!(scene_files(&dir), before, "the undo was not saved");
    rig.caught_up();
    for copy in rig.copies() {
        assert_eq!(copy, before, "a copy did not follow the undo");
    }
    let EditOutcome::Refused { reason, .. } = rig.send(author, &EditOp::Undo) else {
        panic!("an undo past the history applied");
    };
    assert_eq!(reason, EditRefusal::NOTHING_TO_UNDO);

    let (typed, lines) = mpsc::channel();
    for line in ["status", "frobnicate", "quit", "status"] {
        typed.send(line.to_owned()).expect("the console is open");
    }
    let mut printed = Vec::new();
    let now = rig.now + FRAME;
    let last = serve_until_quit(
        &mut rig.server,
        &mut Console::new(lines),
        FRAME,
        || now,
        &mut |line| printed.push(line.to_owned()),
    );
    let [.., status, unknown] = &printed[..] else {
        panic!("the console answered too little: {printed:?}");
    };
    assert!(status.contains("2/8 clients, saved"), "{status}");
    assert_eq!(
        unknown,
        "edit: no command \"frobnicate\"; the commands are status, save, quit"
    );
    assert!(
        last.contains("at revision 2, history at 0 of 1, 0/8 clients"),
        "{last}"
    );
    assert!(
        !rig.printed
            .iter()
            .chain(&printed)
            .any(|line| line.contains("malformed")),
        "clean traffic counted as malformed: {:?}",
        rig.printed
    );

    drop(lock_scene(&dir).expect("the quit let the lock go"));
    rig.until("both clients told the server shut down", |rig| {
        rig.clients.iter().all(|joined| {
            joined.client.ended() == Some(Ended::ByServer(SessionEndReason::SHUTTING_DOWN))
        })
    });
    let reopened = Document::open_with_history(&dir, crcbl_editor::scene::vocabulary())
        .expect("the history is the scene's");
    assert_eq!(
        (reopened.log().position(), reopened.log().len()),
        (0, 1),
        "the history beside the scene is not the move, undone"
    );
}

/// **A drag sent over the protocol is saved as one entry of the history**,
/// decided 2026-10-05: the server saves nothing while the drag's gesture is
/// open — the scene on disk is the committed one until its last frame — and
/// the update that ends it saves the scene and a history of one entry, so
/// after `quit` one `crcbl scene undo` puts back every file the drag moved.
#[test]
fn a_remote_drag_is_saved_as_one_entry_that_one_cli_undo_takes_back() {
    let temp = TempDir::new("drag");
    let dir = field_copy(&temp, "field.scn");
    let expected_dir = field_copy(&temp, "expected.scn");
    let before = scene_files(&dir);
    let mut rig = Rig::serving(&dir);
    let author = rig.join();
    rig.caught_up();

    let xs = [0.5, 1.0, 1.5, 2.0, 2.5];
    for (index, x) in xs.into_iter().enumerate() {
        let last = index + 1 == xs.len();
        let drag = EditGesture { id: 1, last };
        assert!(matches!(
            rig.send_in(author, &EditOp::Apply(move_entry(x)), drag),
            EditOutcome::Applied { .. }
        ));
        if !last {
            assert_eq!(
                scene_files(&dir),
                before,
                "frame {index} was saved mid-drag"
            );
            let status = rig.server.status();
            assert!(status.contains("a drag under way"), "{status}");
        }
    }
    let mut expected =
        Document::open_dir(&expected_dir, crcbl_editor::scene::vocabulary()).expect("the field");
    expected.apply(move_entry(2.5)).expect("a plot moves");
    expected.save().expect("the copy saves");
    assert_eq!(
        scene_files(&dir),
        scene_files(&expected_dir),
        "the drag's end was not saved"
    );
    rig.caught_up();
    for copy in rig.copies() {
        assert_eq!(copy, scene_files(&expected_dir));
    }

    rig.server.quit().expect("saved");
    let reopened = Document::open_with_history(&dir, crcbl_editor::scene::vocabulary())
        .expect("the history is the scene's");
    assert_eq!(
        (reopened.log().position(), reopened.log().len()),
        (1, 1),
        "the drag is not one entry on disk"
    );
    drop(reopened);
    let undo = SceneArgs {
        dir: dir.clone(),
        verb: SceneVerb::Edit(SceneEdit::Undo),
        json: false,
    };
    scene_cmd::run(&undo).expect("the drag undoes");
    assert_eq!(
        scene_files(&dir),
        before,
        "one undo did not take the drag back"
    );
}

/// **While a scene is served, the local edits are refused as locked**: a
/// `crcbl scene` verb and a `crcbl edit` run exit with the lock's code and
/// change nothing, and a second server is refused the same way. Once the
/// server quits, the same verb applies.
#[test]
fn a_local_edit_of_a_served_scene_exits_locked_until_it_quits() {
    let temp = TempDir::new("locked");
    let dir = field_copy(&temp, "field.scn");
    let before = scene_files(&dir);
    let mut rig = Rig::serving(&dir);
    rig.step();

    let set = SceneEdit::Set {
        entity: "4".to_owned(),
        path: "label".to_owned(),
        value: "\"gate\"".to_owned(),
        system: None,
    };
    let scene = SceneArgs {
        dir: dir.clone(),
        verb: SceneVerb::Edit(set.clone()),
        json: false,
    };
    let refused = scene_cmd::run(&scene).expect_err("a served scene is locked");
    assert_eq!(refused.code, EXIT_LOCKED, "{}", refused.message);
    let edit = EditArgs {
        dir: dir.clone(),
        edits: vec![set],
        json: false,
    };
    assert_eq!(
        scene_cmd::run_edit(&edit).expect_err("locked").code,
        EXIT_LOCKED
    );
    assert_eq!(
        Server::open(&dir, loopback())
            .expect_err("a second server")
            .code,
        EXIT_LOCKED
    );
    assert_eq!(scene_files(&dir), before, "a refused edit wrote");

    rig.server.quit().expect("nothing to save");
    scene_cmd::run(&scene).expect("the quit let the scene go");
    assert_ne!(scene_files(&dir), before);
}

/// **A `quit` whose save fails keeps serving, and keeps the edit.** A
/// scene file written behind the server's back — by a program that took no
/// lock — makes the save after the next edit refuse; the status line says
/// the scene is not saved, `quit` says why and holds the lock, and once the
/// server is told to overwrite (`accept_changes_on_disk`) `save` lands the
/// edit and `quit` goes.
#[test]
fn a_quit_whose_save_fails_keeps_serving_and_the_edit() {
    let temp = TempDir::new("unsaved");
    let dir = field_copy(&temp, "field.scn");
    let mut rig = Rig::serving(&dir);
    let author = rig.join();
    rig.caught_up();

    let header = dir.join("scene.ron");
    let mut text = std::fs::read_to_string(&header).expect("a header");
    text.push('\n');
    std::fs::write(&header, &text).expect("a write behind the lock");
    assert_eq!(
        rig.send(author, &EditOp::Apply(move_entry(1.5))),
        EditOutcome::Applied { revision: 1 }
    );
    let status = rig.server.status();
    assert!(status.contains("NOT SAVED: "), "{status}");

    let refused = rig.server.quit().expect_err("an unsaved quit");
    assert!(refused.contains("not quitting"), "{refused}");
    assert!(lock_scene(&dir).is_err(), "a refused quit let the lock go");
    assert_eq!(rig.clients[author].client.ended(), None);

    rig.server.edit.document_mut().accept_changes_on_disk();
    assert_eq!(rig.server.save(), "edit: saved at revision 1");
    let last = rig.server.quit().expect("saved");
    assert!(last.ends_with("saved"), "{last}");
}

/// **Malformed input is counted and never stops the server**: datagrams
/// from no client, a transport whose first message is no hello, and an
/// operation that is no operation — the last refused to its author as
/// malformed — and the next edit still applies. The status line names the
/// count.
#[test]
fn malformed_input_is_counted_and_the_server_serves_on() {
    let temp = TempDir::new("garbage");
    let dir = field_copy(&temp, "field.scn");
    let mut rig = Rig::serving(&dir);
    let author = rig.join();
    rig.caught_up();
    assert_eq!(rig.server.malformed_count(), 0, "clean traffic counted");

    let stray = UdpSocket::bind(loopback()).expect("a loopback socket");
    for datagram in [&[][..], &[0xFF; 40][..], &[0x00, 0x13, 0x37][..]] {
        stray
            .send_to(datagram, rig.server.local_addr())
            .expect("a datagram");
    }
    rig.until("the stray datagrams counted", |rig| {
        rig.server.malformed_count() >= 3
    });

    let counted = rig.server.malformed_count();
    let (mut near, far) = InMemoryTransport::pair();
    rig.server.edit.host_mut().add(Box::new(far));
    near.send_reliable(Message::reliable(vec![0xEE, 0x01, 0x02]))
        .expect("an open pair");
    rig.until("the message that is no hello counted", |rig| {
        rig.server.malformed_count() > counted
    });

    // A move cut short: this build's version, and then less than a command.
    let mut cut = encode_op(&EditOp::Apply(move_entry(1.5))).expect("a move travels");
    cut.truncate(cut.len() / 2);
    let EditOutcome::Refused { reason, .. } = rig.send_bytes(author, cut) else {
        panic!("bytes that are no operation applied");
    };
    assert_eq!(reason, EditRefusal::MALFORMED);
    assert_eq!(
        rig.send(author, &EditOp::Apply(move_entry(1.5))),
        EditOutcome::Applied { revision: 1 }
    );
    let count = rig.server.malformed_count();
    let status = rig.server.status();
    assert!(
        status.ends_with(&format!(", {count} malformed messages refused")),
        "{status}"
    );
}

/// **The help's numbers are the code's**: the status line's interval, and
/// the exit code a held scene exits with.
#[test]
fn the_help_states_the_interval_and_the_lock_code() {
    let usage = crate::scene_args::EDIT_USAGE;
    assert!(
        usage.contains(&format!("every {} seconds", STATUS_INTERVAL.as_secs())),
        "the help names another interval"
    );
    assert!(
        usage.contains(&format!("program holds\n    exits {EXIT_LOCKED}")),
        "the help names another exit for a held scene"
    );
}

/// A player id no other call in this test binary has drawn: every player
/// in one session must be their own, or the host refuses the second as a
/// duplicate.
fn next_player() -> crcbl::net::PlayerId {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    crcbl::net::PlayerId::from_seed(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}
