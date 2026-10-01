//! Play mode: the scene run by its games' modules, and put back exactly as it
//! was when play stops.
//!
//! `docs/plan/08-editor.md`'s decision of 2026-10-01, built:
//!
//! ```text
//!     play  ──▶ files() ─────────────────────────────▶ the snapshot (text)
//!           ──▶ Registry::modules(systems, the snapshot)
//!                 ──▶ a game's refusal, or ──▶ register on the world
//!     each frame, unless paused:
//!           ──▶ FrameClock at the world's tick rate ──▶ world.tick()
//!                                                    ──▶ every module's tick
//!                                                    ──▶ world.sweep()
//!     stop  ──▶ the snapshot ──▶ MemorySource ──▶ the load Document::open runs
//! ```
//!
//! # Why the restore is a load and not a snapshot of the world
//!
//! The 2026-09-16 decision, kept: the load path is the one the engine already
//! tests, and a per-system snapshot loses state silently the day a system
//! forgets to take part. What changed on 2026-10-01 is *where the text comes
//! from* — the scene as it stood when play began, held in memory, rather than
//! the directory on disk — so unsaved edits are no longer lost on play.
//!
//! # What survives a play
//!
//! Everything about the document that is not the world: the undo log, the
//! saved position, the gesture counter and the origin. The log can survive
//! because the restored scene **is** the pre-play scene — every entity under
//! the [`SceneEntityId`](crcbl::scene::scn::SceneEntityId) the save wrote it
//! with — and because the id map's high-water mark is carried across with
//! [`IdMap::reserve`](crcbl::scene::scn::IdMap::reserve): the files spell the
//! ids the scene holds, not the ones a deleted entity's undo still names. The
//! selection survives when the entity it names is in the restored scene.
//!
//! # What a module spawns
//!
//! Towers' creeps: entities in a system the module registered, of a component
//! the vocabulary knows only as
//! [runtime](crcbl::registry::Registry::register_runtime). The document draws
//! them ([`Document::spawned`]) and does nothing else with them — they have no
//! id, so nothing lists, selects, edits or saves them — and stop throws away
//! the world they were spawned in, so none outlives play.
//!
//! # Why every edit is refused in between
//!
//! What play changes is thrown away on stop, so an edit made into a playing
//! scene would be thrown away with it, silently; and a save would write a
//! played state over the authored one. One check, `Document::refuse_in_play`,
//! stands at the top of every public method that writes the scene, the log or
//! the disk — so a path that edits goes through it by construction rather than
//! by remembering to.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use crcbl::core::FrameClock;
use crcbl::ecs::{ClientInputs, GameModule};

use super::{Document, EditError, load, memory_source, sync_scene_colliders};

/// Where play mode stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    /// The scene is being edited, and nothing ticks.
    Editing,
    /// The scene is running, and every edit is refused.
    Playing,
    /// The scene is held where play left it: nothing ticks, and every edit is
    /// still refused, because stopping still throws the played state away.
    Paused,
}

/// The scene as it stood when play began, and what is running it.
pub(super) struct Session {
    /// [`Document::files`] at the moment play began: what stop loads again.
    snapshot: BTreeMap<String, String>,
    /// One fresh instance of every module the vocabulary has for this scene's
    /// systems, in the order they tick.
    modules: Vec<Box<dyn GameModule>>,
    /// The fixed-step accumulator, at the world's tick period.
    clock: FrameClock,
    /// How much frame time play has been handed while not paused: the
    /// timeline [`FrameClock::update`] is fed, so a pause stops it rather than
    /// banking the paused time as ticks owed.
    played: Duration,
    /// Whether ticking is held.
    paused: bool,
}

impl fmt::Debug for Session {
    /// The modules by name, since a [`GameModule`] is not `Debug`, and the
    /// snapshot by its keys — the text is the scene, which a log line does not
    /// want.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("snapshot", &self.snapshot.keys().collect::<Vec<_>>())
            .field(
                "modules",
                &self
                    .modules
                    .iter()
                    .map(|module| module.name())
                    .collect::<Vec<_>>(),
            )
            .field("clock", &self.clock)
            .field("played", &self.played)
            .field("paused", &self.paused)
            .finish()
    }
}

impl Document {
    /// Where play mode stands.
    #[must_use]
    pub fn play_state(&self) -> PlayState {
        match &self.play {
            None => PlayState::Editing,
            Some(session) if session.paused => PlayState::Paused,
            Some(_) => PlayState::Playing,
        }
    }

    /// The names of the modules running the scene, in the order they tick —
    /// empty while editing, and for a scene no registered game plays.
    #[must_use]
    pub fn playing_modules(&self) -> Vec<&str> {
        self.play.as_ref().map_or_else(Vec::new, |session| {
            session.modules.iter().map(|module| module.name()).collect()
        })
    }

    /// Starts play mode, or resumes a paused one; a scene already playing is
    /// left as it is.
    ///
    /// Starting takes the scene's [`files`](Self::files) as the snapshot
    /// [`stop`](Self::stop) restores, builds a fresh instance of every module
    /// the vocabulary registers for the scene's systems from those files
    /// ([`crcbl::registry::Registry::modules`]) and calls each one's
    /// [`register`](GameModule::register) on the document's world. Nothing
    /// ticks until [`advance`](Self::advance) is handed time.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`] if the scene would not save — an entity with no
    /// id, which a save refuses rather than drops — and
    /// [`EditError::Unplayable`] if a game refuses to play the scene, naming
    /// why; in either case play does not start and nothing changed, because
    /// every module is built before any registers. [`EditError::TickRate`] if
    /// a module's `register` left the world a tick period no fixed step can be
    /// taken at; the world is restored from the snapshot first, so the
    /// document is back to editing exactly as it was.
    pub fn play(&mut self) -> Result<(), EditError> {
        if let Some(session) = &mut self.play {
            session.paused = false;
            return Ok(());
        }
        let snapshot = self.files()?;
        // Each game builds its module from the scene's own text, read the way
        // its loader reads it — and refuses here, before anything registers,
        // a scene it would not play.
        let modules = self
            .registry
            .modules(
                self.scene.systems(),
                &memory_source(snapshot.clone())?,
                Path::new(""),
            )
            .map_err(EditError::Unplayable)?;
        for module in &modules {
            module.register(&mut self.world);
        }
        let dt = self.world.tick_dt();
        let Some(period) = Duration::try_from_secs_f64(dt)
            .ok()
            .filter(|period| !period.is_zero())
        else {
            self.restore(&snapshot)?;
            return Err(EditError::TickRate(dt));
        };
        let mut clock = FrameClock::with_period(period);
        // The clock's first update only sets where its timeline starts, so it
        // is given play's zero here and every later update is real time.
        clock.update(Duration::ZERO);
        // A module's `register` may have spawned into the world.
        self.membership += 1;
        self.play = Some(Session {
            snapshot,
            modules,
            clock,
            played: Duration::ZERO,
            paused: false,
        });
        Ok(())
    }

    /// Holds a playing scene where it is: nothing ticks until
    /// [`play`](Self::play) resumes it, and edits are still refused. Returns
    /// whether there was a playing scene to hold.
    pub fn pause(&mut self) -> bool {
        match &mut self.play {
            Some(session) if !session.paused => {
                session.paused = true;
                true
            }
            _ => false,
        }
    }

    /// Ends play mode, putting the scene back exactly as it stood when play
    /// began. Returns whether there was a play to stop.
    ///
    /// See the module docs for what survives: the log, the saved position and
    /// the selection when its entity is still there.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`] if the snapshot would not load again, which is a
    /// writer and reader that disagree — the round trip
    /// `a_load_and_a_save_with_no_edits_is_byte_identical` holds. Play mode is
    /// left running in that case, still refusing edits, rather than handing
    /// back a played state as though it were the scene.
    pub fn stop(&mut self) -> Result<bool, EditError> {
        let Some(session) = self.play.take() else {
            return Ok(false);
        };
        if let Err(error) = self.restore(&session.snapshot) {
            self.play = Some(session);
            return Err(error);
        }
        Ok(true)
    }

    /// Hands play `dt` of frame time, and runs every whole tick it adds up to
    /// at the world's tick period. Returns how many ticks ran: none while
    /// editing or paused.
    ///
    /// Each tick is the server's order (`crates/crcbl-server/src/lib.rs`'s
    /// `Server::tick`): the world's schedule, then every module with
    /// [`ClientInputs::empty`] — the editor has no client to send input — then
    /// a sweep of what the modules despawned. Ticks past the clock's catch-up
    /// cap are dropped rather than owed, which is
    /// [`FrameClock::set_max_catch_up_ticks`]'s spiral-of-death guard: a frame
    /// that stalled does not come back as a burst.
    ///
    /// The colliders are rebuilt afterwards, so a click picks what a module
    /// moved where it is drawn.
    pub fn advance(&mut self, dt: Duration) -> u32 {
        let Some(session) = self.play.as_mut().filter(|session| !session.paused) else {
            return 0;
        };
        session.played = session.played.saturating_add(dt);
        session.clock.update(session.played);
        let entities = self.world.entity_count();
        let mut ticks = 0;
        while session.clock.consume_tick() {
            self.world.tick();
            for module in &mut session.modules {
                module.tick(&mut self.world, ClientInputs::empty());
            }
            self.world.sweep();
            ticks += 1;
        }
        if ticks > 0 {
            sync_scene_colliders(&self.registry, &mut self.world, &self.scene, &self.ids);
        }
        if self.world.entity_count() != entities {
            // Something entered or left: the outliner and the drawn instances
            // re-read on this.
            self.membership += 1;
        }
        ticks
    }

    /// [`EditError::Playing`] in play mode, paused or not, and nothing while
    /// editing — the one check every public method that writes the scene, the
    /// log or the disk makes before it changes anything. See the module docs.
    pub(super) fn refuse_in_play(&self) -> Result<(), EditError> {
        match self.play {
            Some(_) => Err(EditError::Playing),
            None => Ok(()),
        }
    }

    /// Loads `snapshot` into a fresh world through the vocabulary this document
    /// holds, and puts it in place of the one play ran — keeping the id map's
    /// high-water mark and the selection, if its entity is still there.
    fn restore(&mut self, snapshot: &BTreeMap<String, String>) -> Result<(), EditError> {
        let source = memory_source(snapshot.clone())?;
        let (world, scene, mut ids) = load(&source, Path::new(""), &self.registry)?;
        ids.reserve(self.ids.next_id());
        self.world = world;
        self.scene = scene;
        self.ids = ids;
        self.membership += 1;
        self.select(self.selected);
        Ok(())
    }
}
