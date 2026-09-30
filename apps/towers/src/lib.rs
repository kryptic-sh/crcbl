//! Towers — co-op tower defense, and the ladder's flagship.
//!
//! `docs/plan/sample/07-towers.md`, **milestone 1, slices 1, 2 and 3a**: the
//! solo loop on one map, natively and in a browser, with the combat content
//! milestone 1 asks for. The map is scene data — [`scene`] reads it out of a
//! `.scn/` directory the editor opens, which is milestone 2's first step. Three kinds of tower and an upgrade tier each,
//! three kinds of creep, ten scripted waves; creeps walk a path, towers shoot,
//! burst and hold them, kills pay gold, and the run is won or lost. That
//! document's status section is the list of what each remaining slice owes.
//!
//! # What it proves
//!
//! Three things, and every one of them is a `crcbl-phys` L0 query the plan
//! named this sample as the forcing function for:
//!
//! * **A trigger volume is how a creep reaches the exit.**
//!   [`crcbl::phys::PhysicsWorld::set_trigger`] makes the volume non-solid, so
//!   sweeps and rays pass through it and only
//!   [`overlap_sphere`](crcbl::phys::PhysicsWorld::overlap_sphere) reports it —
//!   which is exactly the pair a "did anything get in here?" volume wants.
//!   [`map`] registers it and [`creep::has_reached_the_exit`] asks it.
//! * **Acquisition is a sphere overlap, and the filter is the interesting
//!   half.** The same query hands a tower the ground slab and the exit volume,
//!   so [`tower::acquire`] answers with a creep or with nothing. Slice 3a asks
//!   the same query two more ways: a [`tower::Kind::Splash`] bolt's
//!   [`tower::burst_into`] at the point it lands, which is the plan's "overlap
//!   burst at impact point", and a [`tower::Kind::Slow`] tower's [`tower::hold`]
//!   once a tick, which is a **continuous** reading of the same query where the
//!   other two are instants.
//! * **CCD against a target that is itself moving.** A bolt covers more ground
//!   in one tick than a creep is wide, and the creeps have already walked by
//!   the time the bolts sweep — so
//!   [`sweep_sphere`](crcbl::phys::PhysicsWorld::sweep_sphere) over the
//!   segment is the only thing that sees the hit.
//!   `tower::tests::a_bolt_hits_a_creep_that_a_test_at_either_end_of_the_tick_would_miss`
//!   is that claim with both static readings taken beside it.
//!
//! ```text
//!   keys ──▶ ActionMap ──▶ Controls ──wire──▶ Intent ──▶ Stage
//!                                                          │
//!   WaveSystem ──▶ CreepSystem ──▶ exit trigger ──▶ lives  │
//!        │              │                                  │
//!        │              └──▶ ProjectileSystem ──▶ TowerSystem
//!        └──────────────────────────────▶ EconomySystem ──▶ gold, win, lose
//! ```
//!
//! # It is a real client/server sample
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 has no exemption for a
//! tower defense, and this sample's own document is explicit that solo and
//! co-op are one build: `PlaceTower` and `StartWave` are **commands** the
//! client seals into bytes and the server validates over an
//! `InMemoryTransport`. A refused command is counted, which is how "the server
//! decides" is a number rather than a claim.
//!
//! **And co-op is the same game over UDP.** `--host [PORT]` runs that server on
//! a LAN host whose own player is one of its clients, `--join <IP:PORT>` and
//! `--browse` join one from another machine, and every player's commands are
//! validated against one purse and one pool of lives. The server's world
//! replicates the field as [`replica`]'s entities, which is all a joiner draws
//! from. The `lan` module has the wiring, and it is native only: the browser
//! build is single player.
//!
//! # The path is a polyline, and the engine owes a spline
//!
//! That document asks for creeps that walk a spline. Nothing in `crcbl-phys` or
//! `crcbl-scene` offers a spline type, so [`path`] measures straight legs
//! between the map's waypoints and a creep turns a corner in one tick.
//! It is the one engine gap this slice found, and it is recorded rather than
//! worked around.
//!
//! # The content is three tables and nothing else
//!
//! [`tower::TOWERS`] is every number a tower has, at either tier;
//! [`creep::CREEPS`] is every number a creep has; [`wave::WAVES`] says what each
//! of the ten rows releases and how fast. Each is read in exactly one place and
//! every capacity in [`map`] is derived from them, so a new tower kind is a row
//! and a variant rather than a sweep through the crate — which is the
//! extensibility `docs/plan/sample/07-towers.md`'s exit criteria ask for.
//!
//! # What is not here yet
//!
//! **Slice 3a is the combat half of milestone 1's remaining content, and not the
//! presentation half.** No `.crpix` art and so no build menu worth the name
//! (rule 11 is owed, not exempted); no spatial audio (rule 8 is owed, not
//! exempted); no world-space health bars; no save or resume; and no dev fly/walk
//! camera. There is no pointer or touch input **inside the canvas** either, on
//! the page as well as in the window — [`app`] says why a tap waits for the build
//! menu, and what a touch player gets on the page instead.
//! `docs/plan/sample/07-towers.md` carries the list with what each would take.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here.
//! `src/web.rs` is the second front end — compiled only on `wasm32`, which is
//! why it is not linked on a host build — and it is what the demo site's shim
//! drives once per `requestAnimationFrame`.

pub mod app;
mod args;
pub mod camera;
pub mod creep;
pub mod game;
mod gpu;
#[cfg(not(target_arch = "wasm32"))]
pub mod lan;
pub mod map;
pub mod menu;
pub mod page;
pub mod path;
pub mod replica;
pub mod scene;
pub mod tower;
pub mod wave;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Summary, Towers, TowersError, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use creep::{CREEPS, Creep, CreepSpec, CreepView};
pub use game::{Controls, DEFAULT_TICK_HZ, Game, GameError, RenderState, Stats};
pub use gpu::{Gpu, Paths};
pub use map::{Map, MapError};
pub use menu::{MenuAction, MenuKind, Menus};
pub use page::PageStats;
pub use scene::{FIELD, Plot, Waypoint, built_in_source, register_components};
pub use tower::{Bolt, BoltOutcome, BurstView, TOWERS, Tier, Tower, TowerSpec, TowerView};
pub use wave::{Outcome, Release, WAVES, Wave, Waves};

#[cfg(test)]
mod tests {
    use crcbl_sample_test::browser_gate_expectation as gate;

    use crate::{creep, game, map, tower};

    /// **Every game constant `web/tools/browser-e2e.mjs` writes out is this
    /// crate's, and drift is a red test rather than a red browser row.**
    ///
    /// The gate's `towers` row carries prices, labels and a tick rate beside a
    /// comment naming the symbol each came from, and nothing until this read the
    /// two halves together. The failures that leaves are two, and the second is
    /// the bad one: a changed price reddens the towers row with "the purse did
    /// not pay the cost", which reads as a broken game rather than a stale gate —
    /// and a changed plot label or a changed **maximum bounty** makes one of the
    /// gate's own *controls* wrong, so the row goes on passing while the thing it
    /// was controlling for is no longer true.
    ///
    /// `apps/shard/src/lib.rs` and `apps/breach/src/map/practice.rs` hold the
    /// same kind of mirror for their own rows.
    #[test]
    fn the_browser_gates_game_constants_are_the_ones_this_crate_declares() {
        use tower::Tier::{Base, Upgraded};

        assert_eq!(
            gate("loop", "cost"),
            tower::Kind::Bolt.spec(Base).cost.to_string(),
            "the gate's build price is not a bolt tower's",
        );
        assert_eq!(
            gate("loop", "splashCost"),
            tower::Kind::Splash.spec(Base).cost.to_string(),
            "the gate's splash price is not a splash tower's",
        );
        assert_eq!(
            gate("loop", "splashUpgrade"),
            tower::Kind::Splash.spec(Upgraded).cost.to_string(),
            "the gate's upgrade price is not a splash tower's",
        );

        // The control for every purse reading the row takes after the first
        // kill — see the `maxBounty` comment in that file.
        assert_eq!(
            gate("loop", "maxBounty"),
            creep::CREEPS
                .iter()
                .map(|spec| spec.bounty)
                .max()
                .expect("the creep table has rows")
                .to_string(),
            "the gate's bounty ceiling is not the creep table's",
        );
        assert_eq!(
            gate("loop", "tickHz"),
            game::DEFAULT_TICK_HZ.to_string(),
            "the gate's tick rate is not this sample's",
        );

        // The labels, quoted as the JavaScript spells them. Each is a plot the
        // gate walks its cursor to by **name**, so a renamed or reordered plot
        // in the committed field leaves it pressing keys at a plot it is not on.
        let quoted = |label: &str| format!("'{label}'");
        let map = map::Map::built_in();
        let plots = map.plots();
        assert_eq!(
            gate("loop", "lastPlot"),
            quoted(&plots[plots.len() - 1].label),
            "the gate's far plot is not the committed field's last",
        );
        assert_eq!(
            gate("loop", "emptyPlot"),
            quoted(&plots[plots.len() - 2].label),
            "the gate's empty plot is not the one a second `prev` reaches",
        );
        assert_eq!(
            gate("loop", "kindLabel"),
            quoted(tower::Kind::Splash.label()),
            "the gate's kind label is not the one the `[HUD]` line prints",
        );
        assert_eq!(
            gate("loop", "buttonKindLabel"),
            quoted(tower::Kind::Slow.label()),
            "the gate's button label is not the kind that button's key picks",
        );
    }
}
