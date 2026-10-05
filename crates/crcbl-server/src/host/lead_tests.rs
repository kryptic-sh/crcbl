//! Client tick alignment end to end: a real `crcbl_client::Client` running
//! its input lead against a host's jitter buffer, over links a
//! [`ConditionSimulator`] delays and jitters on a [`ManualClock`], so every
//! run is the same run.

use std::sync::{Arc, Mutex};

use crcbl_client::Client;
use crcbl_client::input_lead::INPUT_LEAD_MARGIN_TICKS;
use crcbl_ecs::System;
use crcbl_net::reliable::EndpointStats;
use crcbl_net::{
    ConditionSimulator, InMemoryTransport, ManualClock, Message, SimConditions, TransportError,
};

use super::tests::{COMPATIBILITY, TICK_HZ};
use super::*;

/// How often both ends are updated: a display frame finer than a tick, so
/// inputs leave and arrive between the host's ticks rather than on them.
const FRAME: Duration = Duration::from_millis(4);

/// One end of a link whose conditions the test can change mid-run: the
/// simulator is shared, so the test keeps a handle to it after the client or
/// the host has taken the transport.
#[derive(Clone)]
struct Link(Arc<Mutex<ConditionSimulator<InMemoryTransport, ManualClock>>>);

impl Link {
    fn sim(&self) -> std::sync::MutexGuard<'_, ConditionSimulator<InMemoryTransport, ManualClock>> {
        self.0.lock().expect("a test link is not poisoned")
    }
}

impl Transport for Link {
    fn send_reliable(&mut self, msg: Message) -> Result<(), TransportError> {
        self.sim().send_reliable(msg)
    }

    fn send_unreliable(&mut self, msg: Message) -> Result<(), TransportError> {
        self.sim().send_unreliable(msg)
    }

    fn recv_reliable(&mut self) -> Result<Option<Message>, TransportError> {
        self.sim().recv_reliable()
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        self.sim().recv()
    }

    fn is_connected(&self) -> bool {
        self.sim().is_connected()
    }

    fn max_unreliable_message_bytes(&self) -> usize {
        self.sim().max_unreliable_message_bytes()
    }

    fn link_stats(&self) -> Option<EndpointStats> {
        self.sim().link_stats()
    }
}

/// What the module was handed of the client's input, tick by tick: the host
/// tick, and the tick each frame it was handed targets.
type Seen = Arc<Mutex<Vec<(u64, Vec<u64>)>>>;

/// A module that records [`Seen`]. The host's first tick is tick 1, and the
/// module is attached before it, so it counts the ticks itself.
struct Record {
    tick: u64,
    seen: Seen,
}

impl HostModule for Record {
    fn tick(&mut self, _world: &mut World, inputs: PeerInputs<'_>) {
        self.tick += 1;
        let frames = inputs
            .iter()
            .flat_map(|(_, frames)| frames.iter().map(|(tick, _)| tick.get()))
            .collect();
        self.seen
            .lock()
            .expect("a test module is not poisoned")
            .push((self.tick, frames));
    }
}

/// A host with one client sending input every tick, over a link delayed the
/// same way both ways.
struct Rig {
    host: Host,
    client: Client<Link>,
    links: [Link; 2],
    clock: ManualClock,
    now: Duration,
    seen: Seen,
}

impl Rig {
    fn new(conditions: &SimConditions) -> Self {
        let mut world = World::new();
        let entity = world.spawn();
        let mut system = System::<f32>::new("position");
        system.attach(entity, 0.0);
        world.register_system(Box::new(system));
        let mut host = Host::new(
            world,
            HostConfig {
                max_peers: 1,
                tick_hz: TICK_HZ,
                compatibility: COMPATIBILITY,
            },
        );
        let seen = Seen::default();
        host.set_module(Box::new(Record {
            tick: 0,
            seen: Arc::clone(&seen),
        }));
        host.update(Duration::ZERO);

        let clock = ManualClock::new();
        let (near, far) = InMemoryTransport::pair();
        let link = |end, seed| {
            let conditions = SimConditions {
                seed,
                ..conditions.clone()
            };
            Link(Arc::new(Mutex::new(ConditionSimulator::with_clock(
                end,
                conditions,
                clock.clone(),
            ))))
        };
        let client_end = link(near, 0xC11E);
        let host_end = link(far, 0x5E1F);
        host.add(Box::new(host_end.clone()));
        let mut client = Client::new_with_compatibility(
            World::new(),
            client_end.clone(),
            TICK_HZ,
            COMPATIBILITY,
        );
        client.set_input(vec![1]);
        Self {
            host,
            client,
            links: [client_end, host_end],
            clock,
            now: Duration::ZERO,
            seen,
        }
    }

    /// Both ends' conditions, from now on.
    fn set_conditions(&mut self, conditions: &SimConditions) {
        for (link, seed) in self.links.iter().zip([0xC11E, 0x5E1F]) {
            link.sim().set_conditions(SimConditions {
                seed,
                ..conditions.clone()
            });
        }
    }

    fn frame(&mut self) {
        self.clock.advance(FRAME);
        self.now += FRAME;
        self.client.update(self.now);
        self.host.update(self.now);
    }

    fn run_for(&mut self, duration: Duration) {
        let until = self.now + duration;
        while self.now < until {
            self.frame();
        }
    }

    /// How far the newest tick the client has stamped runs ahead of the
    /// host's tick right now; `None` before the client has both.
    fn lead_over_host(&self) -> Option<i64> {
        let stamped = self.client.tick_lead()? + self.client.last_applied_tick().get() as i64;
        Some(stamped - self.host.tick_id().get() as i64)
    }

    /// The module's record from host tick `from` on.
    fn seen_from(&self, from: u64) -> Vec<(u64, Vec<u64>)> {
        self.seen
            .lock()
            .expect("a test module is not poisoned")
            .iter()
            .filter(|(tick, _)| *tick >= from)
            .cloned()
            .collect()
    }
}

/// A link's one-way latency, in server ticks.
fn ticks(latency: Duration) -> f64 {
    latency.as_secs_f64() * f64::from(TICK_HZ)
}

/// Long enough for the lead to settle after a change: several feedback
/// windows past a round trip at the slowest link here.
const SETTLE: Duration = Duration::from_secs(4);

/// How long a settled run is measured over.
const MEASURE: Duration = Duration::from_secs(4);

/// **On a steady link the lead converges to half a round trip plus the
/// margin, measured, not guessed.** The handshake runs over a short link and
/// the link then slows, so the lead the handshake's round trip gave is wrong
/// and only the server's timing samples can put it right — the playout's
/// offset moves the wrong way, by the inbound trip, and the outbound trip
/// grows too. Settled, every input is read in the server's last reading
/// before its tick, or — the frames the client sends and the host reads on
/// falling at every phase of a tick, which the worst-of-window lead covers —
/// the one before, and is applied on exactly the tick it names, none late;
/// and the client's newest stamped tick runs the one-way trip plus the
/// margin ahead of the host's, within the tick a whole-tick server reading
/// leaves.
#[test]
fn the_lead_converges_to_half_a_round_trip_plus_the_margin() {
    let short = SimConditions {
        latency: Duration::from_millis(10),
        ..SimConditions::default()
    };
    let slow = SimConditions {
        latency: Duration::from_millis(50),
        ..SimConditions::default()
    };
    let mut rig = Rig::new(&short);
    rig.run_for(Duration::from_secs(1));
    rig.set_conditions(&slow);
    rig.run_for(SETTLE);

    let late_before = rig.host.late_input_count();
    let from = rig.host.tick_id().get() + 1;
    let one_way = ticks(slow.latency);
    let (mut fewest, mut most) = (i64::MAX, i64::MIN);
    let mut zero_margins = 0usize;
    let until = rig.now + MEASURE;
    while rig.now < until {
        rig.frame();
        let lead = rig.lead_over_host().expect("settled");
        fewest = fewest.min(lead);
        most = most.max(lead);
        let margin = rig.client.input_lead_stats().margin_ticks;
        assert!(
            matches!(margin, Some(0 | 1)),
            "an input was read {margin:?} ticks before its tick"
        );
        zero_margins += usize::from(margin == Some(0));
    }
    assert_eq!(
        rig.host.late_input_count(),
        late_before,
        "a settled input was late"
    );
    let seen = rig.seen_from(from);
    assert!(!seen.is_empty());
    for (tick, frames) in &seen {
        assert_eq!(frames, &[*tick], "host tick {tick} was handed {frames:?}");
    }
    let expected = one_way + INPUT_LEAD_MARGIN_TICKS;
    assert!(
        fewest as f64 >= expected.floor() && most as f64 <= expected.ceil() + 1.0,
        "the stamped tick ran {fewest} to {most} ahead of the host's, not about {expected}"
    );
    assert!(
        zero_margins > 0,
        "no input was read in the last reading before its tick"
    );
    assert_eq!(rig.client.input_lead_stats().steps, 0);
}

/// The most of a jittered run's inputs that may arrive late, as a share of
/// those sent: the lead covers the worst arrival of each feedback window,
/// and the margin a tick beyond it, so only an arrival worse than every
/// recent window's is late.
const JITTER_LATE_SHARE: f64 = 0.01;

/// **Under jitter, inputs still apply on the tick they name, arriving in any
/// order, and few are late.** Forty milliseconds each way, give or take
/// fifteen, reorders inputs sent a tick apart; once settled, every frame the
/// module is handed targets the tick it is handed on or, late, the one
/// before it — never a later one, never twice — and the late ones are at
/// most [`JITTER_LATE_SHARE`] of them.
#[test]
fn under_jitter_inputs_apply_on_their_tick_and_few_are_late() {
    let jittered = SimConditions {
        latency: Duration::from_millis(40),
        jitter: Duration::from_millis(15),
        ..SimConditions::default()
    };
    let mut rig = Rig::new(&jittered);
    rig.run_for(SETTLE);

    let late_before = rig.host.late_input_count();
    let from = rig.host.tick_id().get() + 1;
    rig.run_for(MEASURE * 4);
    let late = rig.host.late_input_count() - late_before;

    let seen = rig.seen_from(from);
    let mut handed = 0usize;
    let mut on_their_tick = 0usize;
    let mut targets = std::collections::BTreeSet::new();
    for (tick, frames) in &seen {
        for &target in frames {
            assert!(target <= *tick, "tick {tick} was handed {target} early");
            assert!(targets.insert(target), "tick {target} was handed twice");
            handed += 1;
            if target == *tick {
                on_their_tick += 1;
            }
        }
    }
    assert!(
        handed as f64 >= seen.len() as f64 * 0.9,
        "only {handed} frames reached {} ticks",
        seen.len()
    );
    assert_eq!(
        handed - on_their_tick,
        late as usize,
        "every frame off its tick must be one counted late"
    );
    assert!(
        late as f64 <= handed as f64 * JITTER_LATE_SHARE,
        "{late} of {handed} inputs were late"
    );
    assert_eq!(rig.host.early_input_count(), 0);
}

/// **A peer whose link drops is handed nothing, held inputs included.** A
/// settled client over a slow link always has inputs held at the host for
/// ticks still to come; once its link goes, those are discarded with it, and
/// no later tick hands the lost peer one — `PeerInputs::iter` lists a lost
/// peer with nothing.
#[test]
fn a_lost_peers_held_inputs_go_with_its_link() {
    let slow = SimConditions {
        latency: Duration::from_millis(50),
        ..SimConditions::default()
    };
    let mut rig = Rig::new(&slow);
    // Slow one way only: a simulator holding snapshots for a far end that has
    // gone reports the failed send rather than reading its own end, so the
    // host would never see the link drop.
    rig.links[1].sim().set_conditions(SimConditions::default());
    rig.run_for(SETTLE);
    assert!(
        !rig.seen_from(rig.host.tick_id().get()).is_empty(),
        "the client's inputs were reaching the host"
    );

    // The client moves to a link that leads nowhere, and the rig lets go of
    // the old one, so the host's end of it sees the far end gone.
    let (spare, _unheard) = InMemoryTransport::pair();
    let nowhere = Link(Arc::new(Mutex::new(ConditionSimulator::with_clock(
        spare,
        SimConditions::default(),
        rig.clock.clone(),
    ))));
    rig.client.reconnect(nowhere.clone());
    rig.links[0] = nowhere;
    let from = rig.host.tick_id().get() + 1;
    rig.run_for(Duration::from_millis(200));
    assert!(
        rig.host
            .events()
            .any(|event| matches!(event, PeerEvent::Lost(_)))
    );
    let seen = rig.seen_from(from);
    assert!(!seen.is_empty());
    for (tick, frames) in &seen {
        assert!(
            frames.is_empty(),
            "host tick {tick} handed the lost peer {frames:?}"
        );
    }
}
