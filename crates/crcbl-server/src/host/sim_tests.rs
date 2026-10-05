//! The host's tick boundary for simulation variables, over `InMemoryTransport`
//! with real `crcbl_client::Client`s: who may set one, when it applies, what
//! each asker is told, and the record a replay reproduces the run from.

use std::hash::Hasher;

use crcbl_client::Client;
use crcbl_console::{ConVar, Flags, Table};
use crcbl_ecs::{Access, DebugCtx, Entity, SystemTrait};
use crcbl_net::{ConsoleOutcome, ConsoleReply, InMemoryTransport};

use crcbl_store::MemoryStorage;
use crcbl_store::replay::{FileTransport, ReplayWriter};

use super::sim::NOT_THE_HOST;
use super::tests::{COMPATIBILITY, TICK, TICK_HZ};
use super::*;
use crate::sim_hash::hash_world;

static T_RATE: ConVar = ConVar::new_float(
    "t_rate",
    "Seconds of spin per second of simulation.",
    Flags::SIM,
    0.0,
    8.0,
    1.0,
);
static T_VIEW: ConVar = ConVar::new_bool("t_view", "A knob nothing simulates.", Flags::NONE, false);

fn registry() -> Registry {
    static VARS: &[&ConVar] = &[&T_RATE, &T_VIEW];
    Registry::gather(&[Table::new(VARS, &[], &[])]).expect("distinct names")
}

/// One spinning entity, replicated as its seconds of spin.
struct Spin {
    entity: Entity,
    seconds: f32,
}

impl SystemTrait for Spin {
    fn name(&self) -> &str {
        "spin"
    }

    fn access(&self) -> Access {
        Access::none()
    }

    fn tick(&mut self, _dt: f64) {}

    fn entity_count(&self) -> usize {
        1
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn hash_state(&self, hasher: &mut dyn Hasher) {
        hasher.write_u32(self.seconds.to_bits());
    }

    fn contributes_to_hash(&self) -> bool {
        true
    }

    fn replicate(&self, out: &mut Vec<u8>) -> bool {
        out.extend_from_slice(&self.entity.to_bits().to_le_bytes());
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(&self.seconds.to_le_bytes());
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Spins every tick by the tick's length times `t_rate`, read from the tick's
/// simulation variables.
struct Spinner;

impl HostModule for Spinner {
    fn tick(&mut self, world: &mut World, inputs: PeerInputs<'_>) {
        let step = world.tick_dt() as f32 * inputs.sim_vars().f32(&T_RATE);
        let spin = world.system_mut::<Spin>().expect("the world registers it");
        spin.seconds += step;
    }
}

fn host(with_registry: bool) -> Host {
    let mut world = World::new();
    let entity = world.spawn();
    world.register_system(Box::new(Spin {
        entity,
        seconds: 0.0,
    }));
    let mut host = Host::new(
        world,
        HostConfig {
            max_peers: 4,
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    // The spinner reads `t_rate`, which only a registry puts in the store.
    if with_registry {
        host.set_module(Box::new(Spinner));
        host.set_sim_registry(registry());
    }
    host.update(Duration::ZERO);
    host
}

fn seconds(host: &mut Host) -> f32 {
    host.world_mut()
        .system_mut::<Spin>()
        .expect("the world registers it")
        .seconds
}

/// How far one tick spins at `rate`, computed as the module computes it.
fn step_at(host: &Host, rate: f32) -> f32 {
    host.world().tick_dt() as f32 * rate
}

fn set(name: &str, value: &str) -> ConsoleSet {
    ConsoleSet {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

fn reason(reply: &ConsoleReply) -> &str {
    match &reply.outcome {
        ConsoleOutcome::Refused(reason) => reason,
        ConsoleOutcome::Applied(tick) => panic!("applied at {tick:?}, not refused"),
    }
}

/// A host with its own player and one other client, both in session.
struct Rig {
    host: Host,
    host_player: Client<InMemoryTransport>,
    remote: Client<InMemoryTransport>,
    now: Duration,
}

impl Rig {
    fn new() -> Self {
        let mut host = host(true);
        let (near, far) = InMemoryTransport::pair();
        host.add_host_player(Box::new(far));
        let host_player =
            Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY);
        let (near, far) = InMemoryTransport::pair();
        host.add(Box::new(far));
        let remote = Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY);
        let mut rig = Self {
            host,
            host_player,
            remote,
            now: Duration::ZERO,
        };
        for _ in 0..600 {
            rig.step();
            if rig.host_player.last_applied_tick() > TickId::ZERO
                && rig.remote.last_applied_tick() > TickId::ZERO
            {
                return rig;
            }
        }
        panic!("the clients never applied a snapshot");
    }

    fn step(&mut self) {
        self.step_host();
        self.host_player.update(self.now);
        self.remote.update(self.now);
    }

    fn step_host(&mut self) {
        self.now += TICK;
        assert_eq!(self.host.update(self.now), 1, "one tick a step");
    }

    /// The spin the remote client last applied from a snapshot.
    fn remote_seconds(&self) -> Option<f32> {
        self.remote
            .replicated("spin")
            .next()
            .map(|(_, data)| f32::from_le_bytes(data.try_into().expect("an f32 a spin entry")))
    }
}

#[test]
fn the_host_players_set_applies_at_the_next_tick_boundary_and_not_before() {
    let mut rig = Rig::new();
    rig.host_player
        .send_console_set(&set("T_RATE", "2"))
        .expect("in session");
    // Sent, not yet read: no tick boundary has passed.
    assert_eq!(rig.host.sim_vars().f32(&T_RATE), 1.0);
    assert!(rig.host.sim_record().is_empty());

    let before = seconds(&mut rig.host);
    rig.step();
    let tick = rig.host.tick_id();
    assert_eq!(rig.host.sim_vars().f32(&T_RATE), 2.0);
    // The boundary is the tick's start: the module's very first spin after it
    // already turned at the new rate.
    assert_eq!(
        seconds(&mut rig.host),
        before + step_at(&rig.host, 2.0),
        "the tick that read the set spun at its rate"
    );
    assert_eq!(rig.host.sim_record().len(), 1);
    assert_eq!(rig.host.sim_record()[0].tick, tick);
    assert_eq!(rig.host.sim_record()[0].set.to_string(), "t_rate 2");

    // The reply comes back sealed, naming the variable as declared.
    rig.step();
    let replies: Vec<ConsoleReply> = rig.host_player.console_replies().collect();
    assert_eq!(
        replies,
        [ConsoleReply {
            name: "t_rate".to_owned(),
            value: "2".to_owned(),
            outcome: ConsoleOutcome::Applied(tick),
        }]
    );

    // And the other client sees the cube spin at the new rate.
    rig.run_until_remote_moves();
    let first = rig.remote_seconds().expect("replicated");
    rig.step();
    rig.step();
    let second = rig.remote_seconds().expect("replicated");
    assert!(
        (second - first - 2.0 * step_at(&rig.host, 2.0)).abs() < 1e-5,
        "two ticks at twice the rate: {first} -> {second}"
    );
}

impl Rig {
    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.step();
        }
    }

    /// Steps until the remote client's replicated spin has moved past where
    /// the host's world was when this was called — the snapshot carrying the
    /// new rate has arrived.
    fn run_until_remote_moves(&mut self) {
        let target = seconds(&mut self.host);
        for _ in 0..60 {
            self.step();
            if self.remote_seconds().is_some_and(|s| s >= target) {
                return;
            }
        }
        panic!("the remote client never caught up");
    }
}

#[test]
fn a_client_that_is_not_the_host_is_refused_and_told_why() {
    let mut rig = Rig::new();
    rig.remote
        .send_console_set(&set("t_rate", "3"))
        .expect("in session");
    rig.run(2);
    assert_eq!(rig.host.sim_vars().f32(&T_RATE), 1.0, "not applied");
    assert!(rig.host.sim_record().is_empty(), "not recorded");
    let replies: Vec<ConsoleReply> = rig.remote.console_replies().collect();
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(reason(&replies[0]), NOT_THE_HOST);
    assert_eq!(replies[0].name, "t_rate");
    assert!(
        rig.host_player.console_replies().next().is_none(),
        "only the asker is answered"
    );
}

#[test]
fn unknown_non_sim_and_bad_values_are_refused_by_name() {
    let mut host = host(true);
    for (name, value) in [
        ("t_nope", "1"),
        ("t_view", "1"),
        ("help", "1"),
        ("t_rate", "fast"),
        ("t_rate", "99"),
    ] {
        host.submit_console_set(set(name, value));
    }
    host.update(TICK);
    let reasons: Vec<String> = host
        .take_console_replies()
        .iter()
        .map(|reply| reason(reply).to_owned())
        .collect();
    assert_eq!(
        reasons,
        [
            "unknown variable `t_nope`",
            "`t_view` is not a simulation variable",
            "`help` is a command, not a variable",
            "`t_rate`: `fast` is not a number",
            "`t_rate`: 99 is outside 0..=8",
        ]
    );
    assert_eq!(host.sim_vars().f32(&T_RATE), 1.0);
    assert!(host.sim_record().is_empty());
}

#[test]
fn a_host_given_no_registry_refuses_every_set() {
    let mut host = host(false);
    host.submit_console_set(set("t_rate", "2"));
    host.update(TICK);
    let replies = host.take_console_replies();
    assert_eq!(
        reason(&replies[0]),
        "this host takes no simulation variables"
    );
}

#[test]
fn two_sets_in_one_tick_apply_in_the_order_they_arrived() {
    let mut host = host(true);
    host.submit_console_set(set("t_rate", "3"));
    host.submit_console_set(set("t_rate", "2"));
    let before = seconds(&mut host);
    host.update(TICK);
    assert_eq!(host.sim_vars().f32(&T_RATE), 2.0, "the later set wins");
    let recorded: Vec<String> = host
        .sim_record()
        .iter()
        .map(|applied| applied.set.to_string())
        .collect();
    assert_eq!(recorded, ["t_rate 3", "t_rate 2"]);
    assert_eq!(seconds(&mut host), before + step_at(&host, 2.0));
}

#[test]
fn a_console_set_between_updates_waits_for_the_next_tick() {
    let mut host = host(true);
    host.submit_console_set(set("t_rate", "4"));
    // No tick is due: the clock has not moved.
    assert_eq!(host.update(Duration::ZERO), 0);
    assert_eq!(host.sim_vars().f32(&T_RATE), 1.0);
    assert!(host.take_console_replies().is_empty());
    host.update(TICK);
    assert_eq!(host.sim_vars().f32(&T_RATE), 4.0);
}

/// Runs `host` for `ticks` ticks from where it stands, setting `t_rate` to
/// each `(tick, value)` from the console just before that tick.
fn run(host: &mut Host, ticks: u32, sets: &[(u32, &str)]) {
    let start = host.tick_id().get();
    for step in 1..=ticks {
        for (at, value) in sets {
            if *at == step {
                host.submit_console_set(set("t_rate", value));
            }
        }
        let now = TICK * u32::try_from(start).expect("a short run") + TICK * step;
        assert_eq!(host.update(now), 1);
    }
}

/// **Taking the record drains it**: each take answers the sets applied since
/// the one before, in order, and leaves the record empty, so a recorder that
/// takes after every update holds the host's record to the sets since then.
#[test]
fn taking_the_sim_record_answers_each_set_once_and_leaves_it_empty() {
    let mut host = host(true);
    run(&mut host, 3, &[(1, "2"), (1, "3")]);
    let taken = host.take_sim_record();
    let values: Vec<String> = taken
        .iter()
        .map(|applied| applied.set.to_string())
        .collect();
    assert_eq!(values, ["t_rate 2", "t_rate 3"]);
    assert!(host.sim_record().is_empty(), "the take drained it");
    assert!(host.take_sim_record().is_empty(), "nothing twice");

    run(&mut host, 2, &[(2, "4")]);
    let taken = host.take_sim_record();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].set.to_string(), "t_rate 4");
    assert_eq!(taken[0].tick, host.tick_id());
    // The values the sets left are untouched by taking their record.
    assert_eq!(host.sim_vars().f32(&T_RATE), 4.0);
}

#[test]
fn a_replayed_record_reproduces_the_final_state_hash_bit_for_bit() {
    const TICKS: u32 = 90;
    let mut recorded = host(true);
    run(&mut recorded, TICKS, &[(30, "2.5"), (61, "0.1")]);
    let record = recorded.sim_record().to_vec();
    assert_eq!(record.len(), 2);
    let expected = hash_world(recorded.world(), recorded.tick_id());

    let mut replayed = host(true);
    replayed.replay_sim_record(record.clone());
    run(&mut replayed, TICKS, &[]);
    assert_eq!(hash_world(replayed.world(), replayed.tick_id()), expected);
    assert_eq!(
        replayed.sim_record(),
        record,
        "the replay recorded the same"
    );
    assert_eq!(
        seconds(&mut replayed).to_bits(),
        seconds(&mut recorded).to_bits()
    );

    // The record is what made it match: the same run without it differs.
    let mut unreplayed = host(true);
    run(&mut unreplayed, TICKS, &[]);
    assert_ne!(
        hash_world(unreplayed.world(), unreplayed.tick_id()),
        expected
    );
}

#[test]
fn a_record_entry_for_a_tick_already_passed_is_refused() {
    let mut recorded = host(true);
    run(&mut recorded, 5, &[(2, "3")]);
    let record = recorded.sim_record().to_vec();

    let mut late = host(true);
    run(&mut late, 5, &[]);
    late.replay_sim_record(record);
    run(&mut late, 1, &[]);
    let replies = late.take_console_replies();
    assert_eq!(replies.len(), 1);
    assert!(
        reason(&replies[0]).starts_with("recorded for tick "),
        "{replies:?}"
    );
    assert_eq!(late.sim_vars().f32(&T_RATE), 1.0);
}

/// Records `host`'s run of `ticks` ticks with `sets` into a `.crpl` file the
/// way a recorder would — a state hash at the end of every tick, then the
/// applied sets as text — and reads it back.
fn record_to_file(
    host: &mut Host,
    ticks: u32,
    sets: &[(u32, &str)],
    keep_sets: bool,
) -> FileTransport {
    let mut writer = ReplayWriter::new(TICK_HZ);
    for step in 1..=ticks {
        // `run` counts its steps from one, so this step's sets are due at 1.
        let due: Vec<(u32, &str)> = sets
            .iter()
            .filter(|(at, _)| *at == step)
            .map(|(_, value)| (1, *value))
            .collect();
        run(host, 1, &due);
        writer.push_state_hash(host.tick_id(), hash_world(host.world(), host.tick_id()));
    }
    if keep_sets {
        for applied in host.sim_record() {
            writer.push_sim_set(
                applied.tick,
                ConsoleSet {
                    name: applied.set.name().to_owned(),
                    value: applied.set.value_text(),
                },
            );
        }
    }
    let storage = MemoryStorage::new();
    let path = std::path::Path::new("session.crpl");
    writer.write(&storage, path).expect("a valid recording");
    FileTransport::open(&storage, path).expect("it reads back")
}

fn file_sets(file: &FileTransport) -> Vec<(TickId, ConsoleSet)> {
    file.sim_sets()
        .iter()
        .map(|recorded| (recorded.tick, recorded.set.clone()))
        .collect()
}

fn file_hashes(file: &FileTransport) -> Vec<(TickId, u64)> {
    file.state_hashes()
        .iter()
        .map(|recorded| (recorded.tick, recorded.hash))
        .collect()
}

#[test]
fn a_session_resimulated_from_its_file_reproduces_every_recorded_hash() {
    const TICKS: u32 = 90;
    let mut recorded = host(true);
    let file = record_to_file(&mut recorded, TICKS, &[(30, "2.5"), (61, "0.1")], true);
    assert_eq!(file.sim_sets().len(), 2);
    assert_eq!(file.state_hashes().len(), TICKS as usize);
    let expected = hash_world(recorded.world(), recorded.tick_id());

    let mut replayed = host(true);
    assert_eq!(
        replayed.resimulate(file_sets(&file), file_hashes(&file), []),
        Ok(recorded.tick_id())
    );
    assert_eq!(hash_world(replayed.world(), replayed.tick_id()), expected);
    assert_eq!(
        seconds(&mut replayed).to_bits(),
        seconds(&mut recorded).to_bits()
    );
}

#[test]
fn a_file_without_its_sets_diverges_at_the_first_tick_a_set_changed() {
    let mut recorded = host(true);
    let file = record_to_file(&mut recorded, 90, &[(30, "2.5"), (61, "0.1")], false);
    assert!(file.sim_sets().is_empty());
    let first_set = recorded.sim_record()[0].tick;

    let mut replayed = host(true);
    let error = replayed
        .resimulate(file_sets(&file), file_hashes(&file), [])
        .expect_err("the sets are what made the run");
    let ResimError::Diverged {
        tick,
        recorded: hash,
        ..
    } = error
    else {
        panic!("not a divergence: {error}");
    };
    assert_eq!(tick, first_set);
    assert_eq!(hash, file_hashes(&file)[(first_set.get() - 1) as usize].1);
    assert_eq!(replayed.tick_id(), first_set, "stopped where it diverged");
}

#[test]
fn a_recorded_set_this_host_refuses_is_named_before_any_tick_runs() {
    let at = |tick| TickId::from_raw(tick);
    let mut refusing = host(true);
    assert_eq!(
        refusing.resimulate(
            [(at(3), set("t_rate", "2")), (at(4), set("t_nope", "1"))],
            [(at(5), 0)],
            []
        ),
        Err(ResimError::SetRefused {
            tick: at(4),
            name: "t_nope".to_owned(),
            reason: "unknown variable `t_nope`".to_owned(),
        })
    );
    assert_eq!(refusing.tick_id(), TickId::ZERO, "no tick ran");
    run(&mut refusing, 5, &[]);
    assert_eq!(
        refusing.sim_vars().f32(&T_RATE),
        1.0,
        "nothing was scheduled"
    );

    let mut bare = host(false);
    assert!(matches!(
        bare.resimulate([(at(1), set("t_rate", "2"))], [], []),
        Err(ResimError::SetRefused { reason, .. }) if reason == "this host takes no simulation variables"
    ));
}

#[test]
fn a_recorded_tick_this_host_has_passed_is_refused() {
    let at = |tick| TickId::from_raw(tick);
    let mut late = host(true);
    run(&mut late, 5, &[]);
    // A set applies at its tick's start, and tick 5 has started and ended.
    assert_eq!(
        late.resimulate([(at(5), set("t_rate", "2"))], [], []),
        Err(ResimError::TickPassed {
            tick: at(5),
            host_tick: at(5),
        })
    );

    // Hashes out of order: the later one is reached first, and the earlier
    // one can no longer be compared.
    let mut recorded = host(true);
    let file = record_to_file(&mut recorded, 7, &[], false);
    let hashes = file_hashes(&file);
    let mut replayed = host(true);
    assert_eq!(
        replayed.resimulate([], [hashes[6], hashes[5]], []),
        Err(ResimError::TickPassed {
            tick: at(6),
            host_tick: at(7),
        })
    );
}
