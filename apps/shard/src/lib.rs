//! Shard — a torch-lit interior zone, walked in an isometric-ish third person.
//!
//! `docs/plan/sample/15-shard.md`, **milestone 1, slice 1**: the first cut of the
//! web slice of a persistent-world action-RPG, running natively and in a browser
//! from one build.
//!
//! # What it proves
//!
//! **The rasterised twin under real load.** That doc's milestone 1 exists to put
//! *content* through the fallback paths rather than fixtures: "Rasterised
//! lighting, `IndirectPerBatch` geometry, `ArrayPages` materials — every fallback
//! path, because a browser has no ray tracing, no mesh shaders and no bindless."
//! `apps/lantern` and `apps/quarry` are the **acceptance fixtures** for those
//! paths; this is the **load** on them — a zone of modular tiles, a torch over
//! every brazier and a spot over the shrine, more lights than there are shadow
//! slots to give them, screen-space occlusion and reflections, and a baked
//! irradiance volume, all in a dark interior where a mistake in any of them is
//! visible.
//!
//! And it gives the Pages site a 3D flagship. Every browser figure recorded so
//! far comes from a 2D sample, which is the gap
//! `docs/plan/sample/15-shard.md` names.
//!
//! **A third camera rig on one character controller.** `apps/puppet` drives
//! [`crcbl::phys::CharacterController`] from a third-person orbit and
//! `apps/breach` drives it from inside the character's head; [`camera`] is the
//! third, and it is the one whose camera the player barely controls — a fixed
//! elevation, a fixed distance, and a yaw that moves in quarter turns.
//! `crcbl-phys` gained nothing for any of the three.
//!
//! ```text
//!   shell key ──▶ ActionMap ──▶ Controls ──wire──▶ Intent
//!                                                    │
//!    Q / E ──▶ Iso { yaw } ──────────────────────────┤
//!                                                    ▼
//!                          OrbitCamera::walk_direction ──▶ CharacterController::move_and_slide
//!                                                    │
//!                            zone::LAYOUT ──▶ world ─┴──▶ where the character can go
//!
//!    F ──▶ Intent::pickup ──▶ the tick ──▶ Grid::insert ──▶ what they carry
//!                                                    │
//!    I ──▶ panel ──▶ pointer drag ──────────────▶ Grid::move_within
//!
//!    a foe falls ──┬──▶ Stage::experience ──▶ level::level_for ──▶ health_max
//!    a stack taken ┘
//!
//!    L ──▶ torches_lit ──┐
//!                        ├──▶ light::torches(elapsed, lit) ──▶ ForwardRenderer::set_lights
//!    tick clock ─────────┘
//! ```
//!
//! # The zone
//!
//! [`zone`] is one authored table, [`zone::LAYOUT`], and everything else is read
//! off it: a floor slab per open tile, a solid block per wall tile, pillars, a
//! dais to step onto, braziers, and doorways with holes through them. There is
//! no roof, for the reason [`zone`]'s own docs give: the camera is above one. The meshes and the colliders walk the *same* grid, so what looks solid is
//! solid. `docs/plan/sample/15-shard.md` asks for modular tiling pieces
//! deliberately — they are what topic 25's border locking has to hold
//! together — and this is that kit at its first size.
//!
//! # The light
//!
//! [`light`] is the load the sample exists to be. The braziers carry point lights
//! that flicker on the **simulated** clock; the shrine carries a spot whose cone
//! has the corridor doorway's own posts standing in it; and the irradiance volume
//! is gathered by casting rays into the zone's colliders, so a sealed alcove is
//! genuinely dark in the ambient term. Every one of those features already
//! existed — `crates/crcbl-render/src/shadow.rs`, `effects.rs` and `probe.rs` —
//! and none of them gained a line on shard's behalf.
//!
//! # Rule 12, on the target where the fallbacks are not hypothetical
//!
//! [`Paths`] reads the three selectors off the device and the resolved effect set
//! off the renderer, and puts them on the debug panel, the `[HUD]` heartbeat and
//! the summary line. `docs/plan/sample/15-shard.md` says path reporting "matters
//! here more than anywhere, because this is the sample where the fallback paths
//! carry real content", and a browser's frame goes through `IndirectPerBatch`,
//! `ArrayPages` and `LightingPath::Rasterised` by construction.
//!
//! # The loot loop, and what it teaches
//!
//! [`loot`] is the item table, the grid the character carries and the two rolls
//! that decide what a felled foe leaves and at which [`loot::Rarity`]; [`panel`]
//! is the grid drawn — each stack's footprint outlined in its tier — with a
//! pointer drag built out of `crcbl-ui`'s press capture. [`level`] is what a
//! felled foe and a taken find are *worth*: a table of thresholds, and a level
//! that deepens the character's health pool. Everything about *where an item
//! fits* is [`crcbl::inventory`] — `docs/plan/34-inventory.md`'s kit, of which
//! this sample is the first consumer — and **not one line of the engine changed
//! for it**, which is that plan's own exit criterion. What the kit did not offer
//! is filed as a topic-34 finding rather than built here: `docs/backlog.md`
//! carries the list.
//!
//! # What the character keeps
//!
//! [`save`] is where they are, what they have left, how many times they have
//! been put down, which foes are felled and **what they are carrying**, written
//! through
//! [`crcbl::store::save::SaveWriter`]'s container — the platform data directory
//! natively, the Origin Private File System in a browser. Nothing in
//! `crcbl-store` gained a line for it; what is this sample's is the payload
//! inside the one sector and which directory it goes in.
//!
//! # What is not here yet
//!
//! **All six of milestone 1's verbs are here: explore, fight, loot, level, save,
//! resume.** What a level does not yet have is anything to *spend* it on: there
//! is no skill, no stat point and no equipment, and the whole of what a level is
//! worth is a deeper health pool. A tier is likewise not an affix on an item's
//! effect, because nothing here has an effect — no item is equipped, eaten or
//! swung — so what [`loot::Rarity`] scales is what the find teaches. There is no
//! sector streaming and no networking of any kind — the plan says milestone 1
//! ships none, and the loopback here is sample rule 2 rather than a network.
//! `tests/golden.rs` is milestone 1's golden frames per `GeometryPath` — every
//! path is held to the same reference per bearing, and `EXPOSURE` is public so
//! that suite draws the zone at the stop the sample does — but the recorded
//! browser budget and the peak wasm memory figure that criterion asks for
//! beside them are not here. `docs/backlog.md` carries both, with what each
//! would take.
//!
//! One thing is absent from the picture rather than merely from the feature list,
//! and it is deliberate: **the character is a capsule**. It is the *same* capsule
//! [`crcbl::phys::CharacterConfig`] sweeps, so the figure on screen is the shape
//! the physics moved; an authored rig would be a second character system with no
//! animation to drive it, and `apps/puppet` is the sample that owns that seam.
//!
//! # Rule 11 does not apply
//!
//! No `.crpix` art. `docs/plan/sample/15-shard.md` grants this sample an explicit
//! exemption from rule 11 — "the subject is a lit 3D world" — while keeping rules
//! 4 and 12 in full. The overlay is a readout a reviewer checks the picture
//! against, and pixel art in front of it would be showing the wrong system.
//!
//! # One library, two front ends
//!
//! `src/main.rs` is argv and an exit code; everything else is here. `src/web.rs` is
//! the second front end — compiled only on `wasm32`, which is why it is not linked
//! on a host build — and it is what the demo site's shim drives once per
//! `requestAnimationFrame`.

pub mod app;
mod args;
pub mod camera;
pub mod foe;
pub mod game;
mod gpu;
pub mod level;
pub mod light;
pub mod loot;
pub mod menu;
pub mod page;
pub mod panel;
pub mod save;
pub mod zone;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Shard, ShardError, Summary, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use camera::Iso;
pub use foe::{Foe, FoeView, Kind};
pub use game::{Controls, DEFAULT_TICK_HZ, Dropped, Game, GameError, RenderState, Stats};
pub use gpu::{EXPOSURE, Gpu, Paths};
pub use level::{MAX_LEVEL, THRESHOLDS};
pub use loot::{DEFAULT_SEED, GRID_H, GRID_W, LOOT_REACH_M, Rarity};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
pub use panel::PanelStats;
pub use save::{Character, SaveStats, Vault};
pub use zone::Cell;

#[cfg(test)]
mod tests {
    use crcbl_sample_test::browser_gate_expectation as gate;

    use crate::{foe, level, loot, zone};

    /// **Every game constant `web/tools/browser-e2e.mjs` writes out is this
    /// crate's, and drift is a red test rather than a red browser row.**
    ///
    /// The gate's `EXPECTATIONS` tree writes each one out beside a comment
    /// naming the symbol it came from, and nothing until now read the two
    /// halves together. The failures that leaves are two, and the second is the
    /// bad one: a changed threshold or a changed drop value reddens the shard
    /// row with "the number is not the one the rules give", which reads as a
    /// broken game rather than a stale gate — and a changed [`foe::FOES`] or
    /// [`foe::HEALTH_MAX`] makes the gate's own **control** wrong, so it goes on
    /// passing while the thing it was controlling for is no longer true.
    ///
    /// Compared as the JavaScript spells it rather than as parsed numbers, so
    /// the labels and their order are pinned too: a table that named the tiers
    /// in another order would still add up and would answer a different find.
    #[test]
    fn the_browser_gates_game_constants_are_the_ones_this_crate_declares() {
        let kill = [foe::Kind::Husk, foe::Kind::Adept, foe::Kind::Warden]
            .map(|kind| format!("{}: {}", kind.label(), kind.experience()))
            .join(", ");
        assert_eq!(
            gate("loot", "kill"),
            format!("{{ {kill} }}"),
            "the gate's kill table is not foe::Kind::experience"
        );

        let find = loot::Rarity::ALL
            .map(|tier| format!("{}: {}", tier.label(), tier.experience()))
            .join(", ");
        assert_eq!(
            gate("loot", "find"),
            format!("{{ {find} }}"),
            "the gate's find table is not loot::Rarity::experience"
        );

        let thresholds = level::THRESHOLDS.map(|total| total.to_string()).join(", ");
        assert_eq!(
            gate("loot", "thresholds"),
            format!("[{thresholds}]"),
            "the gate's level table is not level::THRESHOLDS"
        );

        // `{:?}` rather than a parse and a comparison: it is how Rust spells an
        // `f64` that is a whole number — `6.0`, not `6` — which is the spelling
        // the driver uses, and it keeps a float out of an equality test.
        assert_eq!(
            gate("loot", "reach"),
            format!("{:?}", loot::LOOT_REACH_M),
            "the gate's pickup radius is not loot::LOOT_REACH_M"
        );
        assert_eq!(
            gate("save", "spawnAlong"),
            format!("{:?}", zone::spawn().z),
            "the gate's fresh-boot position is not zone::LAYOUT's spawn"
        );

        // Both blocks that carry a control, because a check that read one would
        // pass on a driver where only the other had gone stale.
        for block in ["fight", "save"] {
            assert_eq!(
                gate(block, "count"),
                foe::FOES.to_string(),
                "the gate's {block} block does not post foe::FOES foes"
            );
            assert_eq!(
                gate(block, "full"),
                foe::HEALTH_MAX.to_string(),
                "the gate's {block} block does not start at foe::HEALTH_MAX"
            );
        }
    }
}
