//! Orbit — the physics pillar's acceptance test, wearing a rocket costume.
//!
//! `docs/plan/sample/06-orbit.md`. Launch from a planet surface, punch through
//! the atmosphere, reach a stable orbit, timewarp along it, and come back down.
//! One planet, one moon, one rocket.
//!
//! # What it proves
//!
//! Everything in `docs/plan/05-physics.md`'s L1 row at once, and none of it
//! reimplemented here: point gravity and quadratic atmospheric drag under a
//! symplectic integrator while the engine is running, analytic Kepler
//! propagation while it is not, a reference-frame hierarchy with
//! sphere-of-influence crossings between the planet and its moon, and the
//! bubble that hands a ship from one to the other. See [`game`].
//!
//! # It is a real client/server sample
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 has no exemption for a
//! physics demo: the flight is a [`crcbl::ecs::GameModule`] the authoritative
//! server owns, stepped on the fixed timestep, with a client on the other end
//! of an `InMemoryTransport`. Timewarp is a control the client sends like any
//! other, which is what makes it a server command rather than a rendering
//! trick.
//!
//! # It flies itself until you take the controls
//!
//! A page that has just loaded takes no input, and a rocket standing on a pad
//! is indistinguishable from a stopped loop. So a script flies the ascent — a
//! gravity turn and a circularisation burn — and the first thing the player
//! asks for ends it for good, the same arrangement `apps/viewer` uses for its
//! turntable.
//!
//! # What is here so far
//!
//! `06-orbit.md`'s milestones 1 and 2: the ascent, the orbit and timewarp with
//! its auto-drop, drawn as a map view over the flight instruments. The moon's
//! frame exists and a ship that reached it would be handed over, but nothing
//! flies there yet, and the bodies are drawn as a map rather than in 3D.
//! `docs/backlog.md` records both.

pub mod app;
mod args;
pub mod game;
mod gpu;
pub mod menu;
pub mod page;

#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{Loop, Orbit, OrbitError, PendingLoop, Summary, run, start, with_shell};
pub use args::{Invocation, Options, USAGE, parse};
pub use game::{
    AIR, Controls, DEFAULT_TICK_HZ, DRY_MASS, FUEL_MASS, FlightStats, Game, GameError,
    HEARTBEAT_TICKS, MAX_THRUST, MOON_MU, MOON_ORBIT, MOON_RADIUS, PATH_SAMPLES, PLANET_MU,
    PLANET_RADIUS, Phase, RenderState, SUBSTEPS, TARGET_APOAPSIS, WARP_RATES,
};
pub use menu::{MenuKind, Menus};
pub use page::PageStats;
