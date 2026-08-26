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
//!                          walk_direction ──▶ CharacterController::move_and_slide
//!                                                    │
//!                            zone::LAYOUT ──▶ world ─┴──▶ where the character can go
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
//! deliberately — they are what `docs/plan/25-lod.md`'s border locking has to hold
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
//! # What is not here yet
//!
//! **Slice 1 is one verb, and the verb is explore.** Milestone 1's loop is
//! "explore, fight, loot, level, save, resume"; there is no enemy, no ability, no
//! item, no rarity, no experience, no inventory grid, no save and no OPFS. There
//! is no sector streaming and no networking of any kind — the plan says milestone
//! 1 ships none, and the loopback here is sample rule 2 rather than a network. The
//! golden frames per `GeometryPath` that milestone 1's exit criteria ask for are
//! not here either, and neither is the recorded browser budget or the peak wasm
//! memory figure. `docs/backlog.md` carries all of it, with what each would take.
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
pub mod light;
pub mod menu;
pub mod page;
pub mod zone;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, PendingLoop, Shard, ShardError, Summary, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use camera::{Iso, walk_direction};
pub use foe::{Foe, FoeView, Kind};
pub use game::{Controls, DEFAULT_TICK_HZ, Game, GameError, RenderState, Stats};
pub use gpu::{Gpu, Paths};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
pub use zone::Cell;
