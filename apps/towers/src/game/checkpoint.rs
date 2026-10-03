//! The stage between waves, taken and put back: what a save holds of a
//! [`Stage`], and when a wave's end is worth an autosave.
//!
//! `crate::save` owns the format and where it is kept; this owns the rule
//! for **when** a stage can be saved — in the build phase, in a run still
//! being played — and how a resumed stage is rebuilt, because both read the
//! stage's own fields.

use std::sync::Arc;

use crate::creep::Creep;
use crate::save::{Checkpoint, SaveError, SavedBolt, SavedBurst, SavedCreep, SavedTower};
use crate::tower::{Bolt, Tower};
use crate::wave::Waves;

use super::{Burst, Stage};

/// Why the stage was not saved — what the player who asked is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotSaved {
    /// A wave is releasing: saves are taken between waves.
    WaveReleasing,
    /// The run is over, and plays itself again in a moment.
    RunOver,
    /// This player is in someone else's session, which only its host saves.
    NotTheHost,
}

impl NotSaved {
    /// What the player is shown.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WaveReleasing => "A WAVE IS COMING IN",
            Self::RunOver => "THE RUN IS OVER",
            Self::NotTheHost => "ONLY THE HOST SAVES",
        }
    }
}

impl Stage {
    /// The stage as a save holds it, or why it cannot be saved now.
    ///
    /// **Between waves, and only then**: no wave releasing and the run still
    /// being played — see `crate::save`'s module docs for why what is on the
    /// field is saved with it.
    pub(super) fn checkpoint(&self) -> Result<Checkpoint, NotSaved> {
        if self.outcome.is_over() {
            return Err(NotSaved::RunOver);
        }
        let Some(due_at) = self.waves.due_between() else {
            return Err(NotSaved::WaveReleasing);
        };
        Ok(Checkpoint {
            map: self.map.fingerprint(),
            runs: self.runs,
            ticks: self.ticks,
            elapsed: self.elapsed,
            gold: self.gold,
            lives: self.lives,
            kills: self.kills,
            leaks: self.leaks,
            shots: self.shots,
            built: self.built,
            built_by_kind: self.built_by_kind,
            upgrades: self.upgrades,
            refused: self.refused,
            wave: self.waves.started(),
            due_at,
            towers: self
                .towers
                .iter()
                .map(|tower| SavedTower {
                    plot: tower.plot(),
                    kind: tower.kind(),
                    tier: tower.tier(),
                    ready_at: tower.ready_at(),
                    fired_at: tower.fired_at(),
                })
                .collect(),
            creeps: self
                .creeps
                .iter()
                .map(|creep| SavedCreep {
                    kind: creep.kind(),
                    along: creep.along(),
                    health: creep.health(),
                    slow: creep.slow(),
                })
                .collect(),
            bolts: self
                .bolts
                .iter()
                .map(|bolt| SavedBolt {
                    id: bolt.id(),
                    at: bolt.at(),
                    heading: bolt.heading(),
                    // By place in the list, which is what a resumed stage can
                    // name: a collider id is the physics world's, and the
                    // resumed world is another.
                    target: self
                        .creeps
                        .iter()
                        .position(|creep| creep.body() == bolt.target()),
                    damage: bolt.damage(),
                    burst_m: bolt.burst_m(),
                })
                .collect(),
            bursts: self
                .bursts
                .iter()
                .map(|burst| SavedBurst {
                    id: burst.id,
                    at: burst.at,
                    radius_m: burst.radius_m,
                    raised_at: burst.raised_at,
                })
                .collect(),
        })
    }

    /// Replaces this stage with the one `checkpoint` holds, on this stage's
    /// map — a fresh physics world with the field in it and a sphere for each
    /// creep, put back in the saved list's order.
    ///
    /// The refusals not yet told stay, as a reset keeps them: they belong to
    /// commands already sent. A restored stage has had no player yet, so an
    /// empty dedicated server holds it rather than throwing it away — see
    /// `run_team_tick`.
    ///
    /// # Errors
    ///
    /// [`SaveError::OtherMap`] for a checkpoint of another map, and
    /// [`SaveError::Invalid`] for a tower on a plot this map lacks or a bolt
    /// aimed at a creep the checkpoint does not have — which a checkpoint of
    /// this map, held to it by [`crate::save::decode`], never has.
    pub(super) fn restore(&mut self, checkpoint: &Checkpoint) -> Result<(), SaveError> {
        if checkpoint.map != self.map.fingerprint() {
            return Err(SaveError::OtherMap);
        }
        let Some(waves) = Waves::between(checkpoint.wave, checkpoint.due_at) else {
            return Err(SaveError::Invalid(format!(
                "wave count, {}, is past the table",
                checkpoint.wave
            )));
        };
        let mut towers = Vec::with_capacity(checkpoint.towers.len());
        for saved in &checkpoint.towers {
            let Some(plot) = self.map.plots().get(saved.plot) else {
                return Err(SaveError::Invalid(format!(
                    "tower stands on plot {}, which the map lacks",
                    saved.plot
                )));
            };
            towers.push(Tower::restored(
                saved.plot,
                plot.at(),
                saved.kind,
                saved.tier,
                saved.ready_at,
                saved.fired_at,
            ));
        }

        let refusals = std::mem::take(&mut self.refusals);
        let mut stage = Self::new(Arc::clone(&self.map));
        let creeps: Vec<Creep> = checkpoint
            .creeps
            .iter()
            .zip(0..)
            .map(|(saved, id)| {
                Creep::restored(
                    &mut stage.world,
                    stage.map.path(),
                    saved.kind,
                    saved.along,
                    saved.health,
                    saved.slow,
                )
                .numbered(id)
            })
            .collect();
        let mut bolts = Vec::with_capacity(checkpoint.bolts.len());
        for saved in &checkpoint.bolts {
            // A bolt whose creep has gone is aimed at something that is not a
            // creep, which is what a stale id is in the run that saved: the
            // exit volume, which never becomes one.
            let target = match saved.target {
                None => stage.exit,
                Some(index) => match creeps.get(index) {
                    Some(creep) => creep.body(),
                    None => {
                        return Err(SaveError::Invalid(format!(
                            "bolt {} is aimed at creep {index}, which is not on the field",
                            saved.id
                        )));
                    }
                },
            };
            bolts.push(Bolt::restored(
                saved.id,
                saved.at,
                saved.heading,
                target,
                saved.damage,
                saved.burst_m,
            ));
        }
        stage.towers = towers;
        // Numbered from zero, as they were restored: an id only has to tell
        // apart the creeps on the field, and a save does not keep them.
        stage.released = creeps.len() as u64;
        stage.creeps = creeps;
        stage.bolts = bolts;
        stage.bursts = checkpoint
            .bursts
            .iter()
            .map(|saved| Burst {
                id: saved.id,
                at: saved.at,
                radius_m: saved.radius_m,
                raised_at: saved.raised_at,
            })
            .collect();
        stage.waves = waves;
        stage.gold = checkpoint.gold;
        stage.lives = checkpoint.lives;
        stage.kills = checkpoint.kills;
        stage.leaks = checkpoint.leaks;
        stage.shots = checkpoint.shots;
        stage.built = checkpoint.built;
        stage.built_by_kind = checkpoint.built_by_kind;
        stage.upgrades = checkpoint.upgrades;
        stage.refused = checkpoint.refused;
        stage.refusals = refusals;
        stage.runs = checkpoint.runs;
        stage.ticks = checkpoint.ticks;
        stage.elapsed = checkpoint.elapsed;
        *self = stage;
        Ok(())
    }

    /// Which wave's end this stage is resting at: the run and the waves
    /// started, while it can be saved after at least one wave — what an
    /// autosave is keyed on.
    fn resting_after(&self) -> Option<(u64, usize)> {
        let started = self.waves.started();
        (started > 0 && self.checkpoint().is_ok()).then_some((self.runs, started))
    }
}

/// The autosave cadence: one save at each wave's end — the first tick of the
/// build phase after it, when its last creep has been released. Shard saves
/// on a cadence of simulated time, and a tower defense's natural one is the
/// wave.
#[derive(Debug, Default)]
pub(crate) struct Autosave {
    /// The run and wave count the last checkpoint was taken at, or that the
    /// stage was restored to.
    last: Option<(u64, usize)>,
}

impl Autosave {
    /// The checkpoint to write, when `stage` has just come to rest after a
    /// wave it has not been saved at.
    pub(super) fn due(&mut self, stage: &Stage) -> Option<Checkpoint> {
        let resting = stage.resting_after()?;
        if self.last == Some(resting) {
            return None;
        }
        self.last = Some(resting);
        stage.checkpoint().ok()
    }

    /// A stage was restored to `checkpoint`: its wave's end is the save it
    /// came from, and is not written again.
    pub(crate) fn restored(&mut self, checkpoint: &Checkpoint) {
        self.last = Some((checkpoint.runs, checkpoint.wave));
    }
}

#[cfg(test)]
mod tests;
