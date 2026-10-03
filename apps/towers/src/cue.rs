//! What the field sounds like: the game's events, read off the replicated
//! field, each as a sound at the place it happened.
//!
//! ```text
//!   Game::replicated ──▶ Watcher::hear ──▶ [Cue { sound, at }] ──▶ crate::audio
//!   (the last snapshot,      (what changed
//!    and this one)            between them)
//! ```
//!
//! # Every player hears what their snapshots show
//!
//! A joiner has no stage — rule 2 — so whatever it hears has to come from what
//! the host sends it. That is the replicated field ([`crate::replica`]), and
//! the cues are read off it by **comparing two snapshots**: a tower whose
//! `working` flag rose fired, a creep whose health fell was hit, a creep that
//! is gone while the kill count rose was killed, a new tower on a plot was
//! built. Solo and a host's own player hear the same way, off their own
//! client's reconstruction, so there is one rule for every seat and the
//! stage is never asked anything on audio's behalf.
//!
//! The other way — the stage queueing events for the host to send — was
//! weighed and left: it puts a per-shot message on the reliable channel,
//! which arrives on its own schedule rather than with the snapshot that
//! shows the shot, and it adds a queue to the simulation for a presentation
//! concern. What comparing costs is resolution: two changes between one pair
//! of snapshots are one change. A creep hit and killed between two snapshots
//! a joiner received is heard as a kill, which is what that joiner saw.
//!
//! # What lets a creep be followed from one snapshot to the next
//!
//! Its tag, [`crate::creep::CreepView::tag`]. A creep's place in the field's
//! list moves whenever another is swap-removed, so matching by place would
//! hear the creep that moved into a dead one's slot as the dead one wounded.
//! A burst carries one too, so a burst still being drawn is not heard twice.
//!
//! # When the two snapshots are not one game a moment apart
//!
//! A restart, a run that started itself again, a save loaded on the host and
//! a joiner that stalled all show up as a field that jumped rather than
//! played: the tick count went back, or forward by more than
//! [`LONGEST_HEARD_GAP_S`]. Comparing across a jump would hear every tower a
//! save restored as built and every creep the jump took as killed, so a jump
//! is heard as nothing and the new field is the baseline — and so is the
//! first snapshot, and the first after [`Watcher::forget`].
//!
//! # Nothing here reaches the simulation
//!
//! The watcher reads a [`Decoded`], which is a copy of what the client
//! reconstructed, and answers cues; it holds no handle to a stage and has no
//! way to send a command. `crate::app`'s
//! `hearing_the_field_leaves_the_stage_hash_alone` is that claim held over a
//! full wave.

use crcbl::math::DVec3;

use crate::creep::{self, CreepView};
use crate::map::{MUZZLE_Y, Map};
use crate::replica::Decoded;
use crate::tower::{self, Tier};
use crate::wave::Outcome;

/// How far apart two snapshots may be and still be heard as one game playing,
/// in seconds of simulated time. Past it the field jumped — see the module
/// docs.
///
/// A second, which is far longer than a joiner's snapshots are apart on a
/// link that is working and far shorter than any save or restart moves the
/// clock.
pub const LONGEST_HEARD_GAP_S: f64 = 1.0;

/// Every sound the field makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// A tower of this kind worked: a bolt or splash tower fired, or a slow
    /// tower took hold of something it had not been holding.
    Fire(tower::Kind),
    /// A splash bolt burst.
    Burst,
    /// A creep was hit and lived.
    Hit,
    /// A creep was killed, and its bounty paid.
    Kill,
    /// A creep reached the exit, and the team lost a life.
    Leak,
    /// A wave started.
    Wave,
    /// A tower was built.
    Build,
    /// A tower was stepped up a tier.
    Upgrade,
    /// The server turned down a command of this player's.
    Refused,
    /// The run was won.
    Won,
    /// The run was lost.
    Lost,
}

impl Sound {
    /// Every sound, in [`Sound::index`] order.
    pub const ALL: [Self; 13] = [
        Self::Fire(tower::Kind::Bolt),
        Self::Fire(tower::Kind::Splash),
        Self::Fire(tower::Kind::Slow),
        Self::Burst,
        Self::Hit,
        Self::Kill,
        Self::Leak,
        Self::Wave,
        Self::Build,
        Self::Upgrade,
        Self::Refused,
        Self::Won,
        Self::Lost,
    ];

    /// Where it sits in [`Sound::ALL`]: what `crate::audio` counts it under,
    /// and one less than the id it is banked at.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Fire(kind) => kind.index(),
            Self::Burst => tower::KINDS,
            Self::Hit => tower::KINDS + 1,
            Self::Kill => tower::KINDS + 2,
            Self::Leak => tower::KINDS + 3,
            Self::Wave => tower::KINDS + 4,
            Self::Build => tower::KINDS + 5,
            Self::Upgrade => tower::KINDS + 6,
            Self::Refused => tower::KINDS + 7,
            Self::Won => tower::KINDS + 8,
            Self::Lost => tower::KINDS + 9,
        }
    }

    /// What a test failing on it calls it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fire(kind) => kind.label(),
            Self::Burst => "burst",
            Self::Hit => "hit",
            Self::Kill => "kill",
            Self::Leak => "leak",
            Self::Wave => "wave",
            Self::Build => "build",
            Self::Upgrade => "upgrade",
            Self::Refused => "refused",
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }
}

/// One sound, at the place on the field it happened, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub sound: Sound,
    pub at: DVec3,
}

/// Where a refused command is heard: the plot the player's cursor is on,
/// which is where their eyes were when they asked.
#[must_use]
pub fn refused_at(map: &Map, plot: u8) -> Cue {
    Cue {
        sound: Sound::Refused,
        at: muzzle(map, usize::from(plot)).unwrap_or_else(middle),
    }
}

/// Reads the field's events off the snapshots a client reconstructs — see the
/// module docs.
#[derive(Debug, Default)]
pub struct Watcher {
    /// The snapshot the next one is compared against, or `None` until there
    /// is one.
    last: Option<Decoded>,
}

impl Watcher {
    /// Forgets the field it last heard, so the next snapshot is a baseline:
    /// what a new game — a lobby's pick, a join, a session's end — calls.
    pub fn forget(&mut self) {
        self.last = None;
    }

    /// The cues `now` adds to the field this watcher last heard, on `map`,
    /// ticking at `tick_hz` — and `now` becomes the field it last heard.
    ///
    /// Nothing for the first snapshot, for one no tick later than the last,
    /// and for a field that jumped rather than played — see the module docs.
    pub fn hear(&mut self, now: &Decoded, map: &Map, tick_hz: u32) -> Vec<Cue> {
        let mut cues = Vec::new();
        let Some(last) = self.last.replace(*now) else {
            return cues;
        };
        let longest = (LONGEST_HEARD_GAP_S * f64::from(tick_hz)).ceil() as u64;
        let played = now.stats.runs == last.stats.runs
            && now.stats.ticks > last.stats.ticks
            && now.stats.ticks - last.stats.ticks <= longest;
        if !played {
            return cues;
        }
        towers(&last, now, map, &mut cues);
        creeps(&last, now, map, &mut cues);
        bursts(&last, now, &mut cues);

        let spawn = creep::centre_at(map.path(), 0.0);
        for _ in last.stats.wave..now.stats.wave {
            cues.push(Cue {
                sound: Sound::Wave,
                at: spawn,
            });
        }
        if last.stats.outcome == Outcome::Playing {
            match now.stats.outcome {
                // The run as a whole, so the middle of the field.
                Outcome::Won => cues.push(Cue {
                    sound: Sound::Won,
                    at: middle(),
                }),
                // Where the last life went.
                Outcome::Lost => cues.push(Cue {
                    sound: Sound::Lost,
                    at: map.exit_centre(),
                }),
                Outcome::Playing => {}
            }
        }
        cues
    }
}

/// The middle of the field: what the overhead camera looks at, and where a
/// cue about the run as a whole is heard.
fn middle() -> DVec3 {
    crate::camera::TARGET.as_dvec3()
}

/// Where the tower on `plot` fires from, or `None` for a plot `map` lacks.
fn muzzle(map: &Map, plot: usize) -> Option<DVec3> {
    let feet = map.plots().get(plot)?.at();
    Some(DVec3::new(feet.x, MUZZLE_Y, feet.z))
}

/// A tower built, stepped up, or set working, plot by plot.
fn towers(last: &Decoded, now: &Decoded, map: &Map, cues: &mut Vec<Cue>) {
    let pairs = last.render.towers.iter().zip(&now.render.towers);
    for (plot, (before, after)) in pairs.enumerate() {
        let (Some(after), Some(at)) = (after, muzzle(map, plot)) else {
            continue;
        };
        let sound = match before {
            None => Some(Sound::Build),
            Some(before) if before.kind != after.kind => Some(Sound::Build),
            Some(before) if before.tier == Tier::Base && after.tier == Tier::Upgraded => {
                Some(Sound::Upgrade)
            }
            Some(before) if after.working && !before.working => Some(Sound::Fire(after.kind)),
            Some(_) => None,
        };
        if let Some(sound) = sound {
            cues.push(Cue { sound, at });
        }
    }
}

/// The live creeps of `decoded`.
fn live(decoded: &Decoded) -> &[CreepView] {
    &decoded.render.creeps[..decoded.render.creeps_alive]
}

/// A creep hit, killed or leaking.
///
/// A creep in both snapshots with less health in the second was hit. One in
/// the first only is gone, and the counters say how: every leak is heard at
/// the exit, and the kills at the places the gone creeps were last seen —
/// those furthest from the exit, the nearest having been the ones that
/// leaked. A kill whose creep the first snapshot did not hold is not heard,
/// because there is nowhere to hear it.
fn creeps(last: &Decoded, now: &Decoded, map: &Map, cues: &mut Vec<Cue>) {
    let (before, after) = (live(last), live(now));
    for creep in after {
        if let Some(was) = before.iter().find(|was| was.tag == creep.tag)
            && creep.health < was.health
        {
            cues.push(Cue {
                sound: Sound::Hit,
                at: creep.centre,
            });
        }
    }

    let exit = map.exit_centre();
    let mut gone: Vec<DVec3> = before
        .iter()
        .filter(|was| !after.iter().any(|creep| creep.tag == was.tag))
        .map(|was| was.centre)
        .collect();
    gone.sort_by(|a, b| a.distance(exit).total_cmp(&b.distance(exit)));
    let leaks = now.stats.leaks.saturating_sub(last.stats.leaks);
    let kills = now.stats.kills.saturating_sub(last.stats.kills);
    for _ in 0..leaks {
        cues.push(Cue {
            sound: Sound::Leak,
            at: exit,
        });
    }
    let leaked = usize::try_from(leaks).unwrap_or(usize::MAX);
    let killed = usize::try_from(kills).unwrap_or(usize::MAX);
    for at in gone.into_iter().skip(leaked).take(killed) {
        cues.push(Cue {
            sound: Sound::Kill,
            at,
        });
    }
}

/// A burst the first snapshot was not drawing.
fn bursts(last: &Decoded, now: &Decoded, cues: &mut Vec<Cue>) {
    let before = &last.render.bursts[..last.render.bursts_live];
    for burst in &now.render.bursts[..now.render.bursts_live] {
        if !before.iter().any(|was| was.tag == burst.tag) {
            cues.push(Cue {
                sound: Sound::Burst,
                at: burst.centre,
            });
        }
    }
}

#[cfg(test)]
mod tests;
