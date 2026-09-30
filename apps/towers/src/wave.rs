//! `WaveSystem` and `EconomySystem`: the scripted table, the clock that
//! releases it, and the two numbers a team shares.
//!
//! ```text
//!   idle ──(gap elapsed, or StartWave)──▶ releasing ──(table row spent)──▶ idle
//!     │                                                                     │
//!     └────────────────────(every row spent)──▶ exhausted ──▶ Outcome::Won ─┘
//! ```
//!
//! # A table, not a formula
//!
//! [`WAVES`] is written out row by row. A generated curve is what a game does
//! once it has been played enough to know what the curve should be, and a table
//! is the thing a reviewer can read and disagree with.
//! `docs/plan/sample/07-towers.md` asks for ten waves and these are the ten.
//!
//! # A row says what it sends and how fast, and nothing else
//!
//! [`Wave::mix`] is how many of each [`crate::creep::Kind`] the row releases,
//! and [`Wave::spacing_s`] is how quickly. A creep's health, speed and bounty
//! are its **kind's** — [`crate::creep::CREEPS`] — so there is no per-wave
//! toughness multiplier here and the tenth wave's fast creeps are the first
//! wave's. What escalates is what a row sends: more of them, and more of the
//! kinds that are hard to answer. `the_table_gets_harder_and_the_pool_holds_all_of_it`
//! is that escalation as an assertion over [`Wave::health`] and [`Wave::bounty`]
//! rather than as a claim in this paragraph.
//!
//! **Within a row the kinds go out in [`crate::creep::ALL`]'s order**, and the
//! gap after each release is that creep's own — [`Wave::spacing_s`] times its
//! [`crate::creep::CreepSpec::spacing_scale`]. That is what makes a swarm arrive
//! in a press wherever it appears instead of only in the rows written to be
//! tight.
//!
//! # Doing nothing loses, and that is arithmetic rather than a hope
//!
//! [`STARTING_LIVES`] is deliberately under [`MAX_CREEPS`], the number of
//! creeps the whole table releases — so a run that builds nothing leaks its way
//! to [`Outcome::Lost`] before the table is spent.
//! `a_team_that_builds_nothing_cannot_survive_the_table` asserts the
//! inequality here, and `crate::game`'s
//! `a_field_with_no_towers_on_it_loses_the_run` asserts the outcome.
//!
//! # And the economy is what the late rows are held with
//!
//! The opening purse buys three bolt towers and nothing more. A field that
//! holds the last two rows wants a [`crate::tower::Kind::Splash`] and a
//! [`crate::tower::Kind::Slow`] tower on it and wants them upgraded, which
//! costs several times [`STARTING_GOLD`] — so the bounties the first eight rows
//! pay are not a flourish, they are the only way the plan gets built.
//! `the_bounties_pay_for_the_plan_the_last_rows_need` is that arithmetic, taken
//! off [`WAVES`] and [`crate::tower::TOWERS`] rather than argued here, and
//! `crate::game`'s `neither_a_splash_nor_a_slow_tower_alone_can_hold_the_last_row`
//! is the other half of it: the same five plots played out three ways that are
//! overrun on the tenth row against one that is not.
//!
//! # `StartWave` is a real command with a real refusal
//!
//! [`Waves::start_now`] is what the client's `StartWave` intent reaches, and it
//! answers `false` for the two cases the server must refuse: a wave already
//! releasing, and a table already spent. Waves also start **on their own**
//! after [`GAP_S`], so a field nobody is playing still plays — the command
//! brings the next one forward rather than being the only way to see one.

use crate::creep::{ALL, KINDS, Kind};

/// One row of the scripted table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    /// What the overlay and a failing test call it.
    pub label: &'static str,
    /// How many of each [`Kind`] it releases, indexed by
    /// [`Kind::index`].
    pub mix: [u32; KINDS],
    /// How long between one release and the next, in seconds, **before** the
    /// released creep's own [`crate::creep::CreepSpec::spacing_scale`].
    pub spacing_s: f64,
}

impl Wave {
    /// How many creeps it releases, all kinds counted.
    #[must_use]
    pub const fn creeps(&self) -> u32 {
        let mut total = 0;
        let mut row = 0;
        while row < KINDS {
            total += self.mix[row];
            row += 1;
        }
        total
    }

    /// How many hit points it puts on the field.
    ///
    /// **What this row costs a field to clear**, and the measure the table
    /// escalates on: a row of six tanky creeps is harder than a row of twenty
    /// swarm creeps even though it is smaller.
    #[must_use]
    pub const fn health(&self) -> u32 {
        let mut total = 0;
        let mut row = 0;
        while row < KINDS {
            total += self.mix[row] * CREEP_HEALTH[row];
            row += 1;
        }
        total
    }

    /// What clearing it pays, in gold.
    #[must_use]
    pub const fn bounty(&self) -> u32 {
        let mut total = 0;
        let mut row = 0;
        while row < KINDS {
            total += self.mix[row] * CREEP_BOUNTY[row];
            row += 1;
        }
        total
    }

    /// Which kind the `index`-th creep of this row is, or `None` past its end.
    ///
    /// The mix in [`ALL`]'s order: every fast creep, then every tanky one, then
    /// the swarm.
    #[must_use]
    pub const fn kind_at(&self, index: u32) -> Option<Kind> {
        let mut seen = 0;
        let mut row = 0;
        while row < KINDS {
            seen += self.mix[row];
            if index < seen {
                return Some(ALL[row]);
            }
            row += 1;
        }
        None
    }

    /// How long after the `index`-th creep before the next one, in seconds.
    ///
    /// `None` past the row's end, where what follows is [`GAP_S`] rather than a
    /// spacing.
    #[must_use]
    pub const fn gap_after(&self, index: u32) -> Option<f64> {
        match self.kind_at(index) {
            Some(kind) => Some(self.spacing_s * kind.spec().spacing_scale),
            None => None,
        }
    }
}

/// Each kind's health, in [`ALL`]'s order, for [`Wave::health`].
///
/// A flattened copy of a column of [`crate::creep::CREEPS`] rather than a
/// second set of numbers: `Wave::health` is a `const fn` and cannot call
/// [`Kind::spec`] through a slice index in one, so the column is lifted out
/// here. `the_flattened_columns_are_the_creep_table` is what holds the two
/// together.
const CREEP_HEALTH: [u32; KINDS] = [
    crate::creep::CREEPS[0].health,
    crate::creep::CREEPS[1].health,
    crate::creep::CREEPS[2].health,
];

/// Each kind's bounty, in [`ALL`]'s order. See [`CREEP_HEALTH`].
const CREEP_BOUNTY: [u32; KINDS] = [
    crate::creep::CREEPS[0].bounty,
    crate::creep::CREEPS[1].bounty,
    crate::creep::CREEPS[2].bounty,
];

/// The ten waves, in the order they are released.
///
/// Read the `mix` as `[fast, tanky, swarm]`. The first three rows are the counts
/// slice 1 opened with — fast creeps, and more of them each time — and the seven
/// after them are where the other two archetypes arrive: a press of swarm creeps
/// in `rush`, the first tanky ones in `heavy`, then both together and more of
/// both every row after. `heavy` is the one row whose gap *lengthens*, because
/// what it sends is two tanky creeps rather than a crowd; from `mixed` on the
/// gap shortens every row.
///
/// **The last two rows are what the other two tower kinds exist for.**
/// `crate::game`'s `neither_a_splash_nor_a_slow_tower_alone_can_hold_the_last_row`
/// plays `last` out against three plans that have one of them or neither, and
/// each is overrun.
pub const WAVES: [Wave; 10] = [
    Wave {
        label: "first",
        mix: [4, 0, 0],
        spacing_s: 1.0,
    },
    Wave {
        label: "second",
        mix: [6, 0, 0],
        spacing_s: 0.9,
    },
    Wave {
        label: "third",
        mix: [8, 0, 0],
        spacing_s: 0.85,
    },
    Wave {
        label: "rush",
        mix: [3, 0, 12],
        spacing_s: 0.8,
    },
    Wave {
        label: "heavy",
        mix: [4, 2, 0],
        spacing_s: 0.9,
    },
    Wave {
        label: "mixed",
        mix: [5, 2, 8],
        spacing_s: 0.8,
    },
    Wave {
        label: "press",
        mix: [6, 3, 10],
        spacing_s: 0.75,
    },
    Wave {
        label: "siege",
        mix: [5, 5, 12],
        spacing_s: 0.7,
    },
    Wave {
        label: "flood",
        mix: [14, 12, 28],
        spacing_s: 0.65,
    },
    Wave {
        label: "last",
        mix: [22, 18, 40],
        spacing_s: 0.6,
    },
];

/// How many creeps the whole table releases.
///
/// The size of `crate::map`'s creep instance pool, and therefore the number of
/// creeps that can be on the field at once — the gap between waves is measured
/// from the last **release** rather than from the field clearing, so a wave can
/// still be walking when the next one starts. A generous bound rather than a
/// tight one: nothing measures how many are alive at once, so the only number
/// that cannot be too small is the total.
/// `crate::game`'s `a_splash_and_a_slow_tower_hold_the_whole_table` measures the
/// peak over a whole run against it.
pub const MAX_CREEPS: usize = {
    let mut total = 0;
    let mut row = 0;
    while row < WAVES.len() {
        total += WAVES[row].creeps() as usize;
        row += 1;
    }
    total
};

/// How much gold the whole table pays out, if every creep in it is killed.
///
/// What the purse has to work with over a run, beside [`STARTING_GOLD`] — see
/// `the_bounties_pay_for_the_plan_the_last_rows_need`.
pub const TOTAL_BOUNTY: u32 = {
    let mut total = 0;
    let mut row = 0;
    while row < WAVES.len() {
        total += WAVES[row].bounty();
        row += 1;
    }
    total
};

// Checked while the crate is compiled rather than while its tests run, because
// `Waves::step` has no kind to release out of an empty row and the earliest a
// wrong table can be caught is here.
const _: () = {
    let mut row = 0;
    while row < WAVES.len() {
        assert!(WAVES[row].creeps() > 0, "a row of WAVES releases nothing");
        row += 1;
    }
};

/// How long the build phase is, in seconds: before the first wave, and between
/// one wave's last release and the next wave's first.
pub const GAP_S: f64 = 2.5;

/// What a team starts with, in gold. Three bolt towers' worth — see
/// [`crate::tower::TOWERS`].
pub const STARTING_GOLD: u32 = 120;

/// How many creeps a team can let through before it loses.
pub const STARTING_LIVES: u32 = 12;

/// How the run ended, or that it has not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outcome {
    /// Still being played.
    #[default]
    Playing,
    /// Every wave cleared with lives to spare.
    Won,
    /// The last life went through the exit.
    Lost,
}

impl Outcome {
    /// What the overlay, the debug panel and the `[HUD]` line call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }

    /// Whether the run is over.
    #[must_use]
    pub const fn is_over(self) -> bool {
        !matches!(self, Self::Playing)
    }
}

/// One creep the table let out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Release {
    /// Which row of [`WAVES`] it came from.
    pub wave: usize,
    /// Which archetype was let out.
    pub kind: Kind,
}

/// Where the table has got to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Waves {
    /// How many waves have been started, `0..=WAVES.len()`.
    started: usize,
    /// How many creeps the wave in progress has already released — which is
    /// also **which** creep of its mix goes out next, through
    /// [`Wave::kind_at`].
    released: u32,
    /// When the next release — or, while idle, the next wave — is due, in the
    /// stage's elapsed seconds.
    due_at: f64,
    /// Whether a wave is releasing creeps.
    releasing: bool,
}

impl Default for Waves {
    fn default() -> Self {
        Self::new()
    }
}

impl Waves {
    /// A table nothing has been released from, with the first build phase
    /// already running.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            started: 0,
            released: 0,
            due_at: GAP_S,
            releasing: false,
        }
    }

    /// How many waves have been started.
    #[must_use]
    pub const fn started(&self) -> usize {
        self.started
    }

    /// Whether a wave is releasing creeps this instant.
    #[must_use]
    pub const fn is_releasing(&self) -> bool {
        self.releasing
    }

    /// Whether every row has been released.
    #[must_use]
    pub const fn is_exhausted(&self) -> bool {
        self.started == WAVES.len() && !self.releasing
    }

    /// How long until the next wave starts, in seconds, or `None` while one is
    /// releasing or the table is spent.
    #[must_use]
    pub fn next_in(&self, now: f64) -> Option<f64> {
        (!self.releasing && self.started < WAVES.len()).then(|| (self.due_at - now).max(0.0))
    }

    /// Brings the next wave forward to `now`. Answers whether it was allowed.
    ///
    /// **The server's half of the `StartWave` command.** Refused while a wave
    /// is releasing — there is no "next" to bring forward — and once the table
    /// is spent. `crate::game` counts a refusal, so a rejected command is
    /// something a run reports rather than something it swallows.
    pub fn start_now(&mut self, now: f64) -> bool {
        if self.releasing || self.started >= WAVES.len() {
            return false;
        }
        self.due_at = now;
        true
    }

    /// What the table released on this tick, or `None` on a tick that releases
    /// nothing.
    ///
    /// At most one creep a tick: the shortest gap any row asks of any kind is
    /// far longer than a tick, which
    /// `the_table_never_asks_for_two_creeps_in_one_tick` asserts against the
    /// simulation rate.
    pub fn step(&mut self, now: f64) -> Option<Release> {
        if !self.releasing {
            if self.started >= WAVES.len() || now < self.due_at {
                return None;
            }
            self.started += 1;
            self.released = 0;
            self.releasing = true;
        }
        if now < self.due_at {
            return None;
        }
        let row = self.started - 1;
        let wave = WAVES[row];
        // The const block above is what makes this unreachable: every row
        // releases something, and `released` is below the row's count whenever
        // `releasing` is set.
        let kind = wave
            .kind_at(self.released)
            .unwrap_or_else(|| unreachable!("a releasing row has a creep left to release"));
        // The gap belongs to the creep going out **now**, which is what makes a
        // swarm arrive in a press wherever it appears — read before the counter
        // moves, for exactly that reason.
        let gap = wave
            .gap_after(self.released)
            .unwrap_or_else(|| unreachable!("the creep being released has a gap of its own"));
        self.released += 1;
        if self.released >= wave.creeps() {
            self.releasing = false;
            self.due_at = now + GAP_S;
        } else {
            self.due_at = now + gap;
        }
        Some(Release { wave: row, kind })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creep::CREEPS;
    use crate::tower::{self, Tier};

    /// One tick at the sample's own rate.
    const DT: f64 = 1.0 / crate::game::DEFAULT_TICK_HZ as f64;

    /// Runs the table forward for `seconds`, answering every creep it released.
    fn released(waves: &mut Waves, seconds: f64) -> Vec<(f64, Release)> {
        let mut out = Vec::new();
        let mut now = 0.0;
        while now < seconds {
            now += DT;
            if let Some(release) = waves.step(now) {
                out.push((now, release));
            }
        }
        out
    }

    /// Long enough for every row, every gap and a margin, in seconds.
    fn whole_table_s() -> f64 {
        let releases: f64 = WAVES
            .iter()
            .map(|wave| {
                (0..wave.creeps())
                    .filter_map(|index| wave.gap_after(index))
                    .sum::<f64>()
            })
            .sum();
        releases + (WAVES.len() + 2) as f64 * GAP_S
    }

    /// **The flattened columns are the creep table's own.** [`Wave::health`] and
    /// [`Wave::bounty`] read [`CREEP_HEALTH`] and [`CREEP_BOUNTY`] because a
    /// `const fn` cannot reach [`Kind::spec`] through a loop index, and a copy
    /// is a copy: a changed creep row with these left behind would make every
    /// escalation assertion below agree with a table nothing plays.
    #[test]
    fn the_flattened_columns_are_the_creep_table() {
        for (row, spec) in CREEPS.iter().enumerate() {
            assert_eq!(
                CREEP_HEALTH[row], spec.health,
                "the {} column is not its health",
                spec.label,
            );
            assert_eq!(
                CREEP_BOUNTY[row], spec.bounty,
                "the {} column is not its bounty",
                spec.label,
            );
        }
    }

    /// **A table nobody touches releases exactly what it says**, in order, at
    /// the spacing each row and each kind asks for, and then stops.
    ///
    /// The "and then stops" is the half worth having: a schedule that wrapped
    /// round would look identical for the first three waves and never let a run
    /// be won. The per-kind gap is the other half — a build that used the row's
    /// `spacing_s` flat would release a swarm as slowly as a tanky creep and
    /// pass every other test in this file.
    #[test]
    fn the_table_releases_every_row_once_and_then_stops() {
        let mut waves = Waves::new();
        let all = released(&mut waves, whole_table_s());
        assert_eq!(
            all.len(),
            MAX_CREEPS,
            "the table released {} creeps of {MAX_CREEPS}",
            all.len(),
        );
        assert!(waves.is_exhausted(), "the table is not spent");
        assert_eq!(waves.started(), WAVES.len());

        let mut seen = 0;
        for (row, wave) in WAVES.iter().enumerate() {
            for index in 0..wave.creeps() {
                let (at, release) = all[seen + index as usize];
                assert_eq!(release.wave, row, "creep {seen} came from the wrong row");
                assert_eq!(
                    Some(release.kind),
                    wave.kind_at(index),
                    "creep {index} of the {} wave is a {}",
                    wave.label,
                    release.kind.label(),
                );
                if index > 0 {
                    let gap = at - all[seen + index as usize - 1].0;
                    let asked = wave
                        .gap_after(index - 1)
                        .expect("a creep inside the row has a gap after it");
                    assert!(
                        (gap - asked).abs() < 2.0 * DT,
                        "the {} wave released a {} {gap:.3} s after the one before, not {asked:.3}",
                        wave.label,
                        release.kind.label(),
                    );
                }
            }
            seen += wave.creeps() as usize;
        }

        // The first wave waits out the build phase, and the second waits out
        // another one after the first row is spent.
        assert!(
            (all[0].0 - GAP_S).abs() < 2.0 * DT,
            "the first creep arrived at {:.3} s, not after the {GAP_S} s build phase",
            all[0].0,
        );
        let first_row = WAVES[0].creeps() as usize;
        let between = all[first_row].0 - all[first_row - 1].0;
        assert!(
            (between - GAP_S).abs() < 2.0 * DT,
            "the second wave started {between:.3} s after the first ended, not {GAP_S} s",
        );
    }

    /// **Every archetype is somewhere in the table.** A kind no row releases is
    /// content the game has and nobody ever meets, and its material, its
    /// numbers and its tests would all go on passing.
    #[test]
    fn every_archetype_is_released_by_some_row() {
        for kind in ALL {
            let rows: Vec<&str> = WAVES
                .iter()
                .filter(|wave| wave.mix[kind.index()] > 0)
                .map(|wave| wave.label)
                .collect();
            assert!(
                !rows.is_empty(),
                "no row of the table releases a {}",
                kind.label(),
            );
        }
    }

    /// **`StartWave` brings the next wave forward, and is refused when there is
    /// no next wave to bring.** The refusal is the half the server exists for:
    /// a command accepted while a wave is already releasing would start the
    /// same row twice.
    #[test]
    fn start_wave_is_accepted_when_idle_and_refused_otherwise() {
        let mut waves = Waves::new();
        assert!(waves.start_now(0.0), "an idle table refused the command");
        assert!(
            waves.step(0.0).is_some(),
            "the wave it brought forward did not start",
        );
        assert!(waves.is_releasing());
        assert!(
            !waves.start_now(0.0),
            "a releasing table accepted a second start",
        );

        // Spend the table, then ask again.
        let mut waves = Waves::new();
        released(&mut waves, whole_table_s());
        assert!(waves.is_exhausted());
        assert!(
            !waves.start_now(1_000.0),
            "a spent table accepted another wave",
        );
        assert_eq!(
            waves.started(),
            WAVES.len(),
            "it started a wave past the table",
        );
    }

    /// **The command is worth issuing**: a run that starts every wave the
    /// instant it may finishes the table sooner than one that waits out every
    /// build phase. Without this, `start_now` could return `true` and change
    /// nothing.
    #[test]
    fn starting_waves_early_finishes_the_table_sooner() {
        let seconds = whole_table_s();
        let mut patient = Waves::new();
        let waited = released(&mut patient, seconds);

        let mut eager = Waves::new();
        let mut hurried = Vec::new();
        let mut now = 0.0;
        while now < seconds {
            now += DT;
            eager.start_now(now);
            if let Some(release) = eager.step(now) {
                hurried.push((now, release));
            }
        }
        assert_eq!(hurried.len(), waited.len(), "a different number of creeps");
        let saved = waited[waited.len() - 1].0 - hurried[hurried.len() - 1].0;
        assert!(
            saved > (WAVES.len() - 1) as f64 * GAP_S - 3.0 * DT,
            "starting early saved {saved:.2} s over {} build phases",
            WAVES.len() - 1,
        );
    }

    /// **Every row is at least as hard as the one before it**, measured in the
    /// hit points it puts on the field and the gold it pays — and the pool is
    /// sized to the whole of it.
    ///
    /// Health rather than a creep count, because the count is not the
    /// difficulty: the `heavy` row is six creeps against `rush`'s fifteen and
    /// is half again as much to chew through.
    #[test]
    fn the_table_gets_harder_and_the_pool_holds_all_of_it() {
        for pair in WAVES.windows(2) {
            let (before, after) = (pair[0], pair[1]);
            assert!(
                after.health() >= before.health(),
                "the {} wave puts {} hit points out against the {} wave's {}",
                after.label,
                after.health(),
                before.label,
                before.health(),
            );
            assert!(
                after.bounty() >= before.bounty(),
                "the {} wave pays {} against the {} wave's {}",
                after.label,
                after.bounty(),
                before.label,
                before.bounty(),
            );
        }
        let total: u32 = WAVES.iter().map(Wave::creeps).sum();
        assert_eq!(
            MAX_CREEPS, total as usize,
            "the pool is not the whole table"
        );
        assert_eq!(
            TOTAL_BOUNTY,
            WAVES.iter().map(Wave::bounty).sum::<u32>(),
            "the payout total is not the table's",
        );
        // The last row is the hardest by a margin rather than by a tie, which is
        // what makes it the one a field is built for.
        assert!(
            WAVES[WAVES.len() - 1].health() > 2 * WAVES[0].health(),
            "the last row is not appreciably harder than the first",
        );
    }

    /// **The table never asks for two creeps in one tick**, which is what lets
    /// [`Waves::step`] release at most one per call. Per row **and per kind**,
    /// because the gap is the product of the two.
    #[test]
    fn the_table_never_asks_for_two_creeps_in_one_tick() {
        for wave in WAVES {
            for kind in ALL {
                if wave.mix[kind.index()] == 0 {
                    continue;
                }
                let gap = wave.spacing_s * kind.spec().spacing_scale;
                assert!(
                    gap > DT,
                    "the {} wave releases a {} every {gap:.4} s, inside one {DT} s tick",
                    wave.label,
                    kind.label(),
                );
            }
        }
    }

    /// **A team that builds nothing cannot survive the table**, which is what
    /// makes building the game rather than the decoration. The outcome is
    /// asserted in `crate::game`; this is the arithmetic it rests on.
    #[test]
    fn a_team_that_builds_nothing_cannot_survive_the_table() {
        assert!(
            (STARTING_LIVES as usize) < MAX_CREEPS,
            "{STARTING_LIVES} lives outlast the {MAX_CREEPS} creeps the table releases",
        );
        assert_eq!(
            STARTING_GOLD / tower::Kind::Bolt.spec(Tier::Base).cost,
            3,
            "the opening gold is not three bolt towers",
        );
    }

    /// **The bounties pay for the plan the last rows need, and the opening
    /// purse does not come close.**
    ///
    /// The plan is the one `crate::game`'s
    /// `a_splash_and_a_slow_tower_hold_the_whole_table` actually plays: every
    /// plot built, one of them a [`tower::Kind::Splash`] and one a
    /// [`tower::Kind::Slow`], and all five upgraded. Three clauses, and the
    /// middle one is the point — a plan the opening purse could buy would make
    /// the whole economy decoration.
    #[test]
    fn the_bounties_pay_for_the_plan_the_last_rows_need() {
        use tower::Kind::{Bolt, Slow, Splash};

        let plan = [Bolt, Bolt, Bolt, Splash, Slow];
        assert_eq!(
            plan.len(),
            crate::map::Map::built_in().plots().len(),
            "the plan does not fill the field",
        );
        let cost: u32 = plan
            .iter()
            .map(|kind| kind.spec(Tier::Base).cost + kind.spec(Tier::Upgraded).cost)
            .sum();

        assert!(
            cost > STARTING_GOLD,
            "the plan costs {cost} and the opening purse is {STARTING_GOLD}, so the bounties buy \
             nothing",
        );

        // The first two rows nowhere near pay for it, so the plan is something a
        // run grows into rather than opens with.
        let early = STARTING_GOLD + WAVES[0].bounty() + WAVES[1].bounty();
        assert!(
            cost > early,
            "the plan costs {cost} and two rows' bounties already reach {early}",
        );

        // …and the eight rows before the last two do pay for it, which is what
        // makes the last two holdable at all.
        let earned: u32 = WAVES[..WAVES.len() - 2]
            .iter()
            .map(Wave::bounty)
            .sum::<u32>()
            + STARTING_GOLD;
        assert!(
            cost <= earned,
            "the plan costs {cost} and the first {} rows plus the purse pay {earned}",
            WAVES.len() - 2,
        );
    }
}
