//! The editor joining a served scene, through the loop: an [`EditServer`] in
//! this process on UDP loopback, as `crcbl edit --serve` serves one, the
//! editor joined to it by `--join` or the address typed on Ctrl+O's line, and
//! another client following it as a [`SceneFollower`].
//!
//! The server listens on `127.0.0.1:0` and is stepped a frame at a time on a
//! clock of the test's own, after each editor frame; loopback delivers
//! asynchronously, so each step is followed by a short pause, and every wait
//! is bounded by [`MAX_STEPS`]. This crate cannot run the `crcbl` binary
//! (the CLI depends on the editor), so the server is the `EditServer` it
//! wraps, without its saving: what the server's directory holds here is
//! what the editor wrote, which is nothing.

use super::*;

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::thread;

use crcbl::client::Client;
use crcbl::ecs::World;
use crcbl::net::udp::{UdpListener, UdpTransport};
use crcbl::net::{ProtocolCompatibility, SessionEndReason};
use crcbl::registry::Registry;
use crcbl::scene::edit::{EditOp, encode_op};
use crcbl::scene_edit::serve::{EDIT_PROTOCOL_ID, EDIT_TICK_HZ, edit_compatibility};
use crcbl::scene_edit::{EditServer, SceneFollower};
use crcbl::server::HostConfig;

use super::files::{chord, type_and_enter};
use crate::document::origin_tests::tree;
use crate::document::{HISTORY, lock_scene};

/// The frames each editor here may run: every wait's worth, many times.
const FRAMES: u64 = 20_000;

/// The most steps any wait here runs: ten seconds of the server's time.
const MAX_STEPS: usize = 600;

/// One frame at [`EDIT_TICK_HZ`], the server's step.
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / EDIT_TICK_HZ as u64);

/// The pause after each step, for loopback to deliver.
const PAUSE: Duration = Duration::from_millis(1);

/// The built-in scene's block the edits here move.
const BLOCK: SceneEntityId = SceneEntityId(2);

/// An edit server on loopback, as `crcbl edit --serve` runs one, with a clock
/// of its own.
struct Served {
    edit: EditServer,
    listener: UdpListener,
    now: Duration,
}

impl Served {
    /// `document` served on loopback to clients built with the identifiers
    /// `compatibility` names.
    fn new(document: Document, compatibility: ProtocolCompatibility) -> Self {
        let listener =
            UdpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), EDIT_PROTOCOL_ID)
                .expect("loopback UDP must be available to these tests");
        let mut edit = EditServer::new(
            document,
            HostConfig {
                max_peers: 4,
                tick_hz: EDIT_TICK_HZ,
                compatibility,
            },
        );
        edit.update(Duration::ZERO);
        Self {
            edit,
            listener,
            now: Duration::ZERO,
        }
    }

    /// `document` served to this build's clients.
    fn serving(document: Document) -> Self {
        Self::new(document, edit_compatibility(&crate::scene::vocabulary()))
    }

    fn addr(&self) -> SocketAddr {
        self.listener.local_addr().expect("a bound listener")
    }

    fn step(&mut self) {
        self.now += FRAME;
        while let Some(peer) = self.listener.accept() {
            self.edit.host_mut().add(Box::new(peer));
        }
        self.edit.update(self.now);
        let _ = self.edit.host_mut().events().count();
    }

    fn files(&mut self) -> BTreeMap<String, String> {
        self.edit
            .document_mut()
            .files()
            .expect("the served scene saves")
    }
}

/// Another client of the server, following the scene: what a second editor
/// or a script sees.
struct Watcher {
    client: Client<UdpTransport>,
    follower: SceneFollower,
}

impl Watcher {
    fn to(addr: SocketAddr) -> Self {
        let vocabulary = crate::scene::vocabulary();
        Self {
            client: Client::new_with_compatibility(
                World::new(),
                UdpTransport::connect(addr, EDIT_PROTOCOL_ID).expect("a socket"),
                EDIT_TICK_HZ,
                edit_compatibility(&vocabulary),
                next_player(),
            ),
            follower: SceneFollower::new(vocabulary),
        }
    }

    fn files(&mut self) -> Option<BTreeMap<String, String>> {
        let copy = self.follower.document_mut()?;
        Some(copy.files().expect("the copy saves"))
    }
}

/// An editor, the server it joins, and a watcher once one is added, stepped
/// together.
struct Rig {
    editor: Editor<HeadlessShell>,
    served: Served,
    watcher: Option<Watcher>,
}

impl Rig {
    /// `served`, and an editor started on `options`.
    fn start(served: Served, options: &Options) -> Self {
        let editor = Editor::with_shell(Box::new(HeadlessShell::new()), options)
            .expect("the null backend runs everywhere");
        Self {
            editor,
            served,
            watcher: None,
        }
    }

    /// `served`, and an editor started with `--join` on it.
    fn joining(served: Served) -> Self {
        let mut options = options(FRAMES);
        options.join = Some(served.addr());
        Self::start(served, &options)
    }

    /// `document` served, and an editor started with `--join` on it, its
    /// copy in place.
    fn joined(document: Document) -> Self {
        let mut rig = Self::joining(Served::serving(document));
        rig.until_in_step("the copy lands");
        rig
    }

    fn step(&mut self) {
        self.editor.frame().expect("a frame");
        self.served.step();
        if let Some(watcher) = self.watcher.as_mut() {
            watcher.client.update(self.served.now);
            watcher
                .follower
                .update(&mut watcher.client, self.served.now);
            let _ = watcher.client.edit_replies().count();
        }
        thread::sleep(PAUSE);
    }

    /// Steps until `done` holds, or panics naming `what`.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        for _ in 0..MAX_STEPS {
            self.step();
            if done(self) {
                return;
            }
        }
        panic!("never happened: {what}");
    }

    /// Steps until the editor's copy and the watcher's, if any, are the
    /// server's scene and the server's revision.
    fn until_in_step(&mut self, what: &str) {
        self.until(what, |rig| {
            let served = rig.served.files();
            let joined = rig.editor.joined.as_ref().is_some_and(|joined| {
                joined.follower().revision() == Some(rig.served.edit.revision())
            });
            let watched = rig.watcher.as_mut().is_none_or(|watcher| {
                watcher.follower.revision() == Some(rig.served.edit.revision())
                    && watcher.files().as_ref() == Some(&served)
            });
            joined && watched && rig.editor.document_mut().files().expect("saves") == served
        });
    }

    /// Steps the server alone until `done` holds, or panics naming `what`:
    /// the editor sends nothing meanwhile, so a drag it holds stays open.
    fn serve_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        for _ in 0..MAX_STEPS {
            self.served.step();
            thread::sleep(PAUSE);
            if done(self) {
                return;
            }
        }
        panic!("never happened: {what}");
    }

    fn status(&self) -> (String, Tone) {
        let (text, tone) = self.editor.panels.status();
        (text.to_owned(), tone)
    }
}

/// The compiled-in scene, as the server serves it.
fn built_in() -> Document {
    crate::scene::built_in_document().expect("the compiled-in scene is a scene")
}

/// **An editor started with `--join` fetches the served scene and shows it**:
/// its document is the server's scene byte for byte, routed, and the title
/// and the status line say where it is joined.
#[test]
fn an_editor_joins_a_served_scene_and_shows_its_bytes() {
    let mut rig = Rig::joined(built_in());
    let addr = rig.served.addr();
    assert_eq!(rig.editor.joined_addr(), Some(addr));
    assert!(rig.editor.document().is_routed());
    // And it stays: frames after the copy landed still show it.
    for _ in 0..4 {
        rig.step();
        assert_eq!(
            rig.editor.document_mut().files().expect("saves"),
            rig.served.files()
        );
    }
    let (status, tone) = rig.status();
    assert!(
        status.starts_with("Joined `") && status.contains(&addr.to_string()),
        "{status}"
    );
    assert_eq!(tone, Tone::Info);
    rig.editor.update_title();
    assert!(
        rig.editor.title.contains(&format!("joined {addr}")),
        "{}",
        rig.editor.title
    );
    assert!(!rig.editor.has_unsaved_edits());
}

/// **A drag of a gizmo handle is one gesture**: the server's history gains
/// one entry, the gesture is closed by the release, and the editor's copy and
/// another client's are the server's scene, each with the one entry.
#[test]
fn a_drag_in_a_joined_editor_is_one_entry_on_the_server_and_every_copy() {
    let mut rig = Rig::joined(built_in());
    rig.watcher = Some(Watcher::to(rig.served.addr()));
    rig.until_in_step("the watcher follows");
    let was = rig.served.files();

    rig.editor.document_mut().select(Some(BLOCK));
    rig.editor.frame().expect("a frame");
    let (from, to) = handle_at(&mut rig.editor, gizmo::Grip::Move(gizmo::Axis::X));
    drag(&mut rig.editor, (from + to) * 0.5, to + (to - from) * 0.5);
    rig.until("the drag applies", |rig| {
        rig.served.edit.revision() > 0 && !rig.served.edit.gesture_open()
    });
    rig.until_in_step("every copy follows the drag");

    assert_ne!(rig.served.files(), was, "the drag moved nothing");
    let served = rig.served.edit.document().log();
    assert_eq!(
        (served.len(), served.position()),
        (1, 1),
        "a drag is one entry"
    );
    assert!(
        !rig.served.edit.gesture_open(),
        "the release closed the drag"
    );
    assert_eq!(rig.editor.document().log().len(), 1, "the copy folds alike");
    let watched = rig.watcher.as_ref().expect("a watcher").follower.document();
    assert_eq!(watched.expect("a copy").log().len(), 1);
}

/// **A drag whose release brings a frame sends that frame as its last**: two
/// frames sent while the pointer is held leave the server's gesture open, and
/// the third, made as the pointer comes up, closes it — three notices and one
/// entry, with no frame sent again to end the drag.
#[test]
fn the_frame_a_drag_is_released_on_is_its_last() {
    let mut rig = Rig::joined(built_in());
    let gesture = rig.editor.document_mut().begin_gesture();
    for (frame, x) in [1.5, 2.5].into_iter().enumerate() {
        rig.editor
            .document_mut()
            .apply_in(shift(x), gesture)
            .expect("routed");
        rig.editor.step_join(true);
        rig.serve_until("the held frame applies", |rig| {
            rig.served.edit.revision() == frame as u64 + 1
        });
    }
    assert!(rig.served.edit.gesture_open(), "a held drag is open");

    rig.editor
        .document_mut()
        .apply_in(shift(3.5), gesture)
        .expect("routed");
    rig.editor.step_join(false);
    rig.until("the release closes the drag", |rig| {
        rig.served.edit.revision() == 3 && !rig.served.edit.gesture_open()
    });
    rig.until_in_step("the copy follows the drag");
    for _ in 0..8 {
        rig.step();
    }
    assert_eq!(rig.served.edit.revision(), 3, "a frame was sent again");
    assert_eq!(rig.served.edit.document().log().len(), 1);
}

/// The built-in block moved along x to `x`.
fn shift(x: f64) -> EditCommand {
    EditCommand::SetProperty {
        entity: BLOCK,
        system: crate::scene::BLOCKS.to_owned(),
        path: "position.0".to_owned(),
        value: Value::Float(x),
    }
}

/// **Undo in a joined editor undoes on the server**: a nudge sent and then
/// undone leaves the server's scene as it was, its history standing below
/// the entry, and the editor's copy with it.
#[test]
fn undo_in_a_joined_editor_steps_the_servers_history() {
    let mut rig = Rig::joined(built_in());
    let was = rig.served.files();
    rig.editor.document_mut().select(Some(BLOCK));
    rig.editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
    assert_eq!(
        rig.editor.document_mut().files().expect("saves"),
        was,
        "an edit shows only once the server applies it"
    );
    rig.until("the nudge applies", |rig| rig.served.edit.revision() == 1);
    assert_ne!(rig.served.files(), was);

    rig.editor.act(&Action::Undo);
    rig.until("the undo applies", |rig| rig.served.edit.revision() == 2);
    rig.until_in_step("the copy follows the undo");
    assert_eq!(rig.served.files(), was);
    assert_eq!(rig.served.edit.document().log().position(), 0);
    assert_eq!(rig.editor.document_mut().files().expect("saves"), was);
}

/// **A refused edit shows the server's reason** on the status line, and
/// changes nothing.
#[test]
fn an_edit_the_server_refuses_shows_its_reason() {
    let mut rig = Rig::joined(built_in());
    let was = rig.served.files();
    let wrong = EditCommand::SetProperty {
        entity: BLOCK,
        system: crate::scene::BLOCKS.to_owned(),
        path: "no_such_field".to_owned(),
        value: Value::Float(1.0),
    };
    let reason = built_in()
        .apply(wrong.clone())
        .expect_err("a field the block lacks is refused")
        .to_string();

    rig.editor.document_mut().apply(wrong).expect("routed");
    let expected = format!("Refused: {reason}");
    rig.until("the refusal is said", |rig| rig.status().0 == expected);
    assert_eq!(rig.status().1, Tone::Warning);
    assert_eq!(rig.served.files(), was);
    assert_eq!(rig.served.edit.revision(), 0);
}

/// **Another client's edit updates the joined editor's document**: the
/// watcher moves the block, and the editor's copy moves with the server's.
#[test]
fn a_remote_edit_updates_the_joined_editors_document() {
    let mut rig = Rig::joined(built_in());
    rig.watcher = Some(Watcher::to(rig.served.addr()));
    rig.until("the watcher is in session", |rig| {
        rig.watcher
            .as_ref()
            .is_some_and(|watcher| watcher.client.session_id().is_some())
    });
    let moved = EditOp::Apply(EditCommand::SetProperty {
        entity: BLOCK,
        system: crate::scene::BLOCKS.to_owned(),
        path: "position.0".to_owned(),
        value: Value::Float(7.25),
    });
    rig.watcher
        .as_mut()
        .expect("a watcher")
        .client
        .send_edit(encode_op(&moved).expect("travels"))
        .expect("in session");
    rig.until("the editor follows the remote edit", |rig| {
        rig.editor
            .document_mut()
            .read(BLOCK, crate::scene::BLOCKS, "position.0")
            .ok()
            == Some(Value::Float(7.25))
    });
    rig.until_in_step("every copy is the server's");
}

/// **Save while joined saves nothing here and says the server does**: Ctrl+S
/// opens no save-as line — the copy has no directory, which a Save would
/// otherwise ask for — and the status line names the server.
#[test]
fn save_in_a_joined_editor_says_the_server_saves() {
    let mut rig = Rig::joined(built_in());
    let addr = rig.served.addr();
    chord(&mut rig.editor, Modifiers::CTRL, KeyCode::KeyS);
    assert_eq!(
        rig.editor.panels.saving_as(),
        None,
        "a save-as was asked for"
    );
    let (status, tone) = rig.status();
    assert_eq!(
        status,
        format!("Joined {addr}: the server saves every edit itself, so nothing is saved here")
    );
    assert_eq!(tone, Tone::Info);
    chord(
        &mut rig.editor,
        Modifiers::CTRL | Modifiers::SHIFT,
        KeyCode::KeyS,
    );
    assert_eq!(
        rig.editor.panels.saving_as(),
        None,
        "a save-as was asked for"
    );
}

/// **A joined editor holds no lock and writes nothing into the scene's
/// directory**: an editor that had the directory open, locked, joins the
/// scene served from it by the address typed on the path line, lets the lock
/// go, and — with an edit applied, a Save asked for and its autosave due —
/// leaves the directory as it was and writes no recovery copy.
#[test]
fn a_joined_editor_holds_no_lock_and_writes_nothing() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let dir = towers_field(temp.path());
    let recovery = temp.path().join("recovery");
    let served = Served::serving(
        Document::open_dir(&dir, crate::scene::vocabulary()).expect("towers' field opens"),
    );
    let addr = served.addr();
    let mut options = options(FRAMES);
    options.scene = Some(dir.clone());
    options.recovery = Some(recovery.clone());
    let mut rig = Rig::start(served, &options);
    assert!(lock_scene(&dir).is_err(), "the editor holds what it opened");
    let before = tree(&dir);

    rig.editor.open(&addr.to_string());
    rig.until_in_step("the copy lands");
    assert_eq!(rig.editor.joined_addr(), Some(addr));
    drop(lock_scene(&dir).expect("a joined editor holds no lock"));

    rig.editor.document_mut().select(Some(SceneEntityId(4)));
    rig.editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
    rig.until("the nudge applies", |rig| rig.served.edit.revision() == 1);
    rig.until_in_step("the copy follows");
    rig.editor.act(&Action::Save);
    rig.editor.autosave = crate::app::recovery::Autosave::new(Duration::ZERO);
    for _ in 0..4 {
        rig.step();
    }
    assert_eq!(tree(&dir), before, "the scene's directory changed");
    assert!(!dir.join(HISTORY).exists());
    let copies = std::fs::read_dir(&recovery).map_or(0, Iterator::count);
    assert_eq!(copies, 0, "a recovery copy was written while joined");
}

/// **A server quitting leaves the copy as an unsaved document**: not routed,
/// with no directory, dirty — so closing asks and Save asks for a directory
/// — holding the scene as the server last had it, and the status line says
/// so. Nothing is edited first, so the copy's own history says it is as
/// fetched: what makes it unsaved is the server going.
#[test]
fn a_server_quitting_leaves_an_unsaved_local_copy() {
    let mut rig = Rig::joined(built_in());
    assert!(
        !rig.editor.document().is_dirty(),
        "the copy reads unsaved as fetched"
    );
    let last = rig.served.files();

    rig.served
        .edit
        .host_mut()
        .shutdown(SessionEndReason::SHUTTING_DOWN);
    rig.until("the editor leaves", |rig| {
        rig.editor.joined_addr().is_none()
    });
    assert!(!rig.editor.document().is_routed());
    assert!(rig.editor.document().is_dirty(), "the copy reads saved");
    assert!(rig.editor.has_unsaved_edits());
    assert_eq!(rig.editor.document().origin(), None);
    assert_eq!(rig.editor.document_mut().files().expect("saves"), last);
    let (status, tone) = rig.status();
    assert!(
        status.contains("shut down") && status.contains("unsaved copy"),
        "{status}"
    );
    assert_eq!(tone, Tone::Warning);
}

/// **A build with another vocabulary is refused**, and the status line says
/// so: the server's identifiers digest a vocabulary this build's does not
/// match, so the handshake fails and no scene is fetched.
#[test]
fn a_mismatched_vocabulary_is_refused_with_a_status() {
    let served = Served::new(built_in(), edit_compatibility(&Registry::new()));
    let addr = served.addr();
    let mut rig = Rig::joining(served);
    rig.until("the refusal is said", |rig| {
        rig.editor.joined_addr().is_none()
    });
    let (status, tone) = rig.status();
    assert!(
        status.starts_with(&format!("{addr} refused this editor")),
        "{status}"
    );
    assert_eq!(tone, Tone::Warning);
    assert_eq!(
        rig.editor.document().entity_count(),
        0,
        "a scene was fetched"
    );
    assert!(!rig.editor.document().is_routed());
}

/// **Ctrl+O takes an address**: typed on the path line, it joins the scene
/// served there as `--join` does.
#[test]
fn an_address_typed_on_the_open_line_joins() {
    let served = Served::serving(built_in());
    let addr = served.addr();
    let mut rig = Rig::start(served, &options(FRAMES));
    chord(&mut rig.editor, Modifiers::CTRL, KeyCode::KeyO);
    type_and_enter(&mut rig.editor, &addr.to_string());
    rig.until_in_step("the copy lands");
    assert_eq!(rig.editor.joined_addr(), Some(addr));
}

mod compose;

/// A player id no other call in this test binary has drawn: every player
/// in one session must be their own, or the host refuses the second as a
/// duplicate.
fn next_player() -> crcbl::net::PlayerId {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    crcbl::net::PlayerId::from_seed(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}
