//! How fast the cube spins: the sandbox's one simulation variable, and the
//! cube as a LAN host simulates it.
//!
//! [`sv_spin_rate`] is a `Flags::SIM` console variable — the engine's first.
//! Typing `sv_spin_rate 2` does not write it: the console hands the set to
//! [`HostedGame::submit_sim_set`](crcbl::engine::HostedGame::submit_sim_set),
//! and the sandbox sends it to whichever simulation it runs. Offline that is
//! [`Scene`](crate::scene::Scene)'s, which applies it at the start of its next
//! tick; in a LAN session it is the host's — from the host's own console, or
//! from a client over the transport, which the host refuses because only its
//! own player may set one. Either way the answer is printed to the console.
//!
//! # The cube a host simulates
//!
//! A LAN host's world spins a cube of its own — `HostedSpin`, replicated as
//! its seconds of spin — advanced by `SpinModule` at the rate the tick's
//! boundary left, so a client sees the host's cube turn at the rate the host
//! set. During a session the drawn cube follows that one rather than the
//! sandbox's own scene. Both are native only, as `crate::lan`'s session is:
//! a web build has no networking.

use crcbl::console::{Registry, Table};

crcbl::console::convar! {
    /// Seconds of spin the cube turns through per second of simulation.
    #[flags(SIM)]
    pub static sv_spin_rate: f32 in 0.0..=8.0 = 1.0;
}

/// Everything the sandbox exposes to the console.
pub fn console_table() -> Table {
    crcbl::console::table![sv_spin_rate]
}

/// The registry a simulation checks the sandbox's sets against, and builds its
/// store of simulation variables from.
///
/// # Panics
///
/// Never: [`console_table`] holds one entry, which claims no built-in's name.
pub fn sim_registry() -> Registry {
    Registry::gather(&[console_table()]).expect("the sandbox's table claims no built-in name")
}

#[cfg(not(target_arch = "wasm32"))]
pub use hosted::{HostedSpin, SpinModule, hosted_seconds, replicated_seconds};

#[cfg(not(target_arch = "wasm32"))]
mod hosted {
    use std::hash::Hasher;

    use crcbl::client::Client;
    use crcbl::ecs::{Access, DebugCtx, Entity, SystemTrait, World};
    use crcbl::net::Transport;
    use crcbl::server::{HostModule, PeerInputs};

    use super::sv_spin_rate;

    /// The replicated system holding the host's cube.
    pub const SPIN: &str = crate::scene::SPIN;

    /// The bytes one replicated spin carries: an `f32`, little-endian.
    const SPIN_BYTES: usize = size_of::<f32>();

    /// The host's cube and how far it has spun, replicated as an `f32` of
    /// seconds — `System<T>` replicates nothing, so the cube is a system of
    /// its own.
    #[derive(Debug)]
    pub struct HostedSpin {
        cube: Entity,
        seconds: f32,
    }

    impl HostedSpin {
        /// A cube in `world` that has not spun yet, registered there.
        pub fn install(world: &mut World) {
            let cube = world.spawn();
            world.register_system(Box::new(Self { cube, seconds: 0.0 }));
        }
    }

    impl SystemTrait for HostedSpin {
        fn name(&self) -> &str {
            SPIN
        }

        /// Nothing shared: the tick does nothing, and [`SpinModule`] reaches the
        /// cube between ticks.
        fn access(&self) -> Access {
            Access::none()
        }

        /// Nothing: [`SpinModule`] spins it, because the rate is a tick input
        /// the schedule is not handed.
        fn tick(&mut self, _dt: f64) {}

        fn entity_count(&self) -> usize {
            1
        }

        fn sweep(&mut self, _dead: &[Entity]) {}

        fn debug_draw(&mut self, _ctx: &DebugCtx) {}

        fn hash_state(&self, hasher: &mut dyn Hasher) {
            hasher.write_u64(self.cube.to_bits());
            hasher.write_u32(self.seconds.to_bits());
        }

        fn contributes_to_hash(&self) -> bool {
            true
        }

        fn replicate(&self, out: &mut Vec<u8>) -> bool {
            out.extend_from_slice(&self.cube.to_bits().to_le_bytes());
            out.extend_from_slice(&(SPIN_BYTES as u32).to_le_bytes());
            out.extend_from_slice(&self.seconds.to_le_bytes());
            true
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// Spins the host's cube each tick by the tick's length times
    /// [`sv_spin_rate`], as the tick's boundary left it.
    #[derive(Debug)]
    pub struct SpinModule;

    impl HostModule for SpinModule {
        fn tick(&mut self, world: &mut World, inputs: PeerInputs<'_>) {
            // Narrowed before the multiply, as `Scene::tick` narrows its step,
            // so a host and an offline sandbox spin by the same f32 sum.
            let step = world.tick_dt() as f32;
            let rate = inputs.sim_vars().f32(&sv_spin_rate);
            if let Some(spin) = world.system_mut::<HostedSpin>() {
                spin.seconds += step * rate;
            }
        }
    }

    /// How far the cube in `world` has spun, or `None` for a world without
    /// one.
    pub fn hosted_seconds(world: &mut World) -> Option<f32> {
        world.system_mut::<HostedSpin>().map(|spin| spin.seconds)
    }

    /// How far the host's cube had spun in the snapshot `client` last applied,
    /// or `None` before one arrived — or when the entry is not an `f32`,
    /// which a host of this build never sends.
    pub fn replicated_seconds<T: Transport>(client: &Client<T>) -> Option<f32> {
        let (_, data) = client.replicated(SPIN).next()?;
        let bytes: [u8; SPIN_BYTES] = data.try_into().ok()?;
        Some(f32::from_le_bytes(bytes))
    }
}
