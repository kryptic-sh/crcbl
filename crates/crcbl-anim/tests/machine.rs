//! The animation state machine: the asset's parse and its refusals by name,
//! transitions on conditions and on exit time, the crossfade, events, root
//! motion, and the determinism the server relies on.
//!
//! Most clips here are a power of two seconds long and most steps a power of
//! two fractions of a second, so normalised time is exact and a test can say
//! *which* tick a transition or an event lands on rather than "about when".

use std::hash::{DefaultHasher, Hash, Hasher};

use crcbl_anim::{
    Channel, Clip, EventId, Interpolation, Joint, Machine, MachineError, MachineState,
    ParameterKind, Pose, Sampler, Skeleton, StateMachine, Track, Trs, blend_into,
};
use glam::{Mat4, Quat, Vec3};

/// One step of the tests' fixed timestep: a sixty-fourth of a second, exact in
/// binary, so a one-second clip is sixty-four steps exactly.
const TICK: f32 = 1.0 / 64.0;

/// How far a measured root velocity may sit from the one the clip authors, in
/// metres per second. The arithmetic is exact in the cases below; this is
/// headroom for the interpolation's own rounding and nothing more.
const VELOCITY_TOLERANCE: f32 = 1e-4;

/// How far the `walk` clip carries the root over its one second, in metres
/// along `-Z`.
const WALK_STRIDE: f32 = 2.0;

/// The root and one limb.
fn skeleton() -> Skeleton {
    Skeleton::new(vec![
        Joint {
            parent: None,
            inverse_bind: Mat4::IDENTITY,
            rest: Trs {
                translation: Vec3::new(0.0, 0.5, 0.0),
                ..Trs::IDENTITY
            },
        },
        Joint {
            parent: Some(0),
            inverse_bind: Mat4::IDENTITY,
            rest: Trs::IDENTITY,
        },
    ])
    .expect("the parent precedes the child")
}

/// A clip that turns joint 1 from `from` to `to` radians about `+Z` over
/// `seconds`.
fn swing(seconds: f32, from: f32, to: f32) -> Channel {
    Channel::new(
        1,
        vec![0.0, seconds],
        Interpolation::Linear,
        Track::Rotation(vec![Quat::from_rotation_z(from), Quat::from_rotation_z(to)]),
    )
    .expect("two keyframes and two values")
}

/// The root walking `stride` metres along `-Z` over `seconds`, from its rest
/// height.
fn stride(seconds: f32, stride: f32) -> Channel {
    Channel::new(
        0,
        vec![0.0, seconds],
        Interpolation::Linear,
        Track::Translation(vec![Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.0, 0.5, -stride)]),
    )
    .expect("two keyframes and two values")
}

/// The clips the assets below name.
fn clip(name: &str) -> Option<Clip> {
    Some(match name {
        // One keyframe: a held stance with no cycle.
        "idle" => Clip::new(vec![
            Channel::new(
                1,
                vec![0.0],
                Interpolation::Linear,
                Track::Rotation(vec![Quat::from_rotation_z(0.2)]),
            )
            .expect("one keyframe and one value"),
        ]),
        "walk" => Clip::new(vec![stride(1.0, WALK_STRIDE), swing(1.0, -0.5, 0.5)]),
        "sprint" => Clip::new(vec![stride(0.5, WALK_STRIDE), swing(0.5, -0.9, 0.9)]),
        "leap" => Clip::new(vec![swing(0.5, 0.0, 1.2)]),
        "march" => Clip::new(vec![swing(1.0, -0.3, 0.3)]),
        _ => return None,
    })
}

/// The machine every behavioural test below plays: idle, a run that blends a
/// walk and a sprint by speed, and a jump.
const CHARACTER: &str = r#"
StateMachine(
    parameters: [
        Float(name: "speed", default: 0.0),
        Bool(name: "grounded", default: true),
        Trigger(name: "jump"),
    ],
    initial: "idle",
    states: [
        State(name: "idle", motion: Clip("idle")),
        State(
            name: "run",
            motion: Blend1d(parameter: "speed", stops: [(2.0, "walk"), (4.0, "sprint")]),
            events: [Event(at: 0.0, name: "footstep"), Event(at: 0.5, name: "footstep")],
        ),
        State(name: "jump", motion: Clip("leap"), looping: false),
    ],
    transitions: [
        Transition(from: "idle", to: "jump", when: [Triggered("jump")], crossfade: 0.0),
        Transition(from: "run", to: "jump", when: [Triggered("jump")], crossfade: 0.0),
        Transition(from: "idle", to: "run", when: [Above("speed", 0.5)], crossfade: 0.25),
        Transition(from: "run", to: "idle", when: [Below("speed", 0.5)], crossfade: 0.25),
        Transition(
            from: "jump",
            to: "idle",
            when: [IsTrue("grounded")],
            exit_time: Some(1.0),
            crossfade: 0.0,
        ),
    ],
)
"#;

fn character() -> Machine {
    Machine::new(
        StateMachine::from_ron(CHARACTER).expect("the character asset is well formed"),
        clip,
    )
    .expect("every clip it names exists")
}

/// A machine of one looping state playing `motion`, with `events` on its
/// track and `transitions` out of it.
fn single(motion: &str, events: &str, transitions: &str) -> Machine {
    let text = format!(
        "StateMachine(parameters: [Float(name: \"speed\", default: 2.0)], initial: \"loop\", \
         states: [State(name: \"loop\", motion: {motion}, events: [{events}]), \
         State(name: \"after\", motion: Clip(\"idle\"))], transitions: [{transitions}])"
    );
    Machine::new(
        StateMachine::from_ron(&text).expect("the single-state asset is well formed"),
        clip,
    )
    .expect("every clip it names exists")
}

fn hash_of(state: &MachineState) -> u64 {
    let mut hasher = DefaultHasher::new();
    state.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// The asset
// ---------------------------------------------------------------------------

/// **A well-formed asset parses, and every name in it resolves.**
#[test]
fn the_asset_parses_and_its_names_resolve() {
    let machine = character();
    let asset = machine.asset();
    assert_eq!(asset.state_count(), 3);
    assert_eq!(asset.state_name(asset.initial()), "idle");
    assert_eq!(
        asset.state("jump").map(|s| asset.state_name(s)),
        Some("jump")
    );
    assert_eq!(asset.state("swim"), None);
    assert_eq!(asset.clip_names(), ["idle", "walk", "sprint", "leap"]);
    let footstep = asset.event("footstep").expect("the run carries footsteps");
    assert_eq!(asset.event_name(footstep), "footstep");

    let start = machine.start();
    let speed = asset.float_parameter("speed").expect("declared");
    let grounded = asset.bool_parameter("grounded").expect("declared");
    let jump = asset.trigger_parameter("jump").expect("declared");
    assert_eq!(start.state(), asset.initial());
    assert_eq!(start.float(speed), 0.0);
    assert!(
        start.bool(grounded),
        "a bool starts at its declared default"
    );
    assert!(!start.is_triggered(jump), "a trigger starts clear");
}

/// The text of [`CHARACTER`] with one substring replaced, parsed.
fn edited(from: &str, to: &str) -> Result<StateMachine, MachineError> {
    assert!(
        CHARACTER.contains(from),
        "the edit {from:?} matches nothing"
    );
    StateMachine::from_ron(&CHARACTER.replacen(from, to, 1))
}

/// **A transition to a state nobody declared is refused, by that name.**
#[test]
fn an_unknown_state_is_refused_by_name() {
    let error = edited("to: \"jump\"", "to: \"jmup\"").expect_err("no state is named jmup");
    assert_eq!(
        error,
        MachineError::UnknownState {
            name: "jmup".into()
        }
    );
    assert!(error.to_string().contains("\"jmup\""), "{error}");

    let initial = edited("initial: \"idle\"", "initial: \"rest\"")
        .expect_err("the initial state must exist too");
    assert_eq!(
        initial,
        MachineError::UnknownState {
            name: "rest".into()
        }
    );
}

/// **A condition on a parameter nobody declared is refused, by that name** —
/// and so is a blend driven by one.
#[test]
fn an_unknown_parameter_is_refused_by_name() {
    let error =
        edited("Above(\"speed\"", "Above(\"sped\"").expect_err("no parameter is named sped");
    assert_eq!(
        error,
        MachineError::UnknownParameter {
            name: "sped".into()
        }
    );
    assert!(error.to_string().contains("\"sped\""), "{error}");

    let blend = edited("parameter: \"speed\"", "parameter: \"pace\"")
        .expect_err("the blend's parameter must exist");
    assert_eq!(
        blend,
        MachineError::UnknownParameter {
            name: "pace".into()
        }
    );
}

/// **A negative crossfade is refused, naming the transition.**
#[test]
fn a_negative_crossfade_is_refused_by_name() {
    let error = edited("crossfade: 0.25", "crossfade: -0.25")
        .expect_err("a fade cannot take negative time");
    assert_eq!(
        error,
        MachineError::NegativeCrossfade {
            from: "idle".into(),
            to: "run".into(),
            seconds: -0.25,
        }
    );
    assert!(error.to_string().contains("\"idle\" -> \"run\""), "{error}");
}

/// A parameter used as a kind it is not, and the rest of the shapes that
/// parse and do not hold together.
#[test]
fn the_other_incoherent_shapes_are_refused_by_name() {
    assert_eq!(
        edited("Triggered(\"jump\")", "Triggered(\"speed\")").expect_err("speed is a float"),
        MachineError::WrongParameterKind {
            name: "speed".into(),
            expected: ParameterKind::Trigger,
        }
    );
    assert_eq!(
        edited("State(name: \"jump\"", "State(name: \"idle\"").expect_err("two idles"),
        MachineError::DuplicateState {
            name: "idle".into()
        }
    );
    assert_eq!(
        edited("Event(at: 0.5", "Event(at: 1.5").expect_err("past the cycle"),
        MachineError::EventOutOfRange {
            state: "run".into(),
            event: "footstep".into(),
            at: 1.5,
        }
    );
    assert_eq!(
        edited("exit_time: Some(1.0)", "exit_time: Some(1.25)").expect_err("past the cycle"),
        MachineError::ExitTimeOutOfRange {
            from: "jump".into(),
            to: "idle".into(),
            exit_time: 1.25,
        }
    );
    assert!(matches!(
        edited("(4.0, \"sprint\")", "(1.0, \"sprint\")").expect_err("the stops descend"),
        MachineError::BadBlend { ref state, .. } if state == "run"
    ));
    // An unknown field is ron's to refuse, and the message names it.
    match edited("looping: false", "lopping: false").expect_err("no such field") {
        MachineError::Parse { message, .. } => {
            assert!(message.contains("lopping"), "{message}");
        }
        other => panic!("expected a parse error, got {other:?}"),
    }
}

/// **A state playing a clip the caller does not have is refused at binding**,
/// by the clip's name.
#[test]
fn an_unknown_clip_is_refused_when_the_machine_is_bound() {
    let asset = StateMachine::from_ron(CHARACTER).expect("well formed");
    let error = Machine::new(asset, |name| {
        (name != "sprint").then(|| clip(name)).flatten()
    })
    .expect_err("there is no sprint");
    assert_eq!(
        error,
        MachineError::UnknownClip {
            name: "sprint".into()
        }
    );
}

// ---------------------------------------------------------------------------
// Transitions and the crossfade
// ---------------------------------------------------------------------------

/// **A transition fires when its condition holds, and not before.**
#[test]
fn a_transition_fires_on_its_condition() {
    let machine = character();
    let asset = machine.asset();
    let speed = asset.float_parameter("speed").expect("declared");
    let run = asset.state("run").expect("declared");
    let mut state = machine.start();
    let mut events = Vec::new();

    // Under the threshold: nothing.
    state.set_float(speed, 0.4);
    for _ in 0..10 {
        machine.step(&mut state, TICK, &mut events);
    }
    assert_eq!(state.state(), asset.initial(), "it left idle at 0.4 m/s");

    // Over it: the very next step.
    state.set_float(speed, 0.6);
    machine.step(&mut state, TICK, &mut events);
    assert_eq!(state.state(), run);
    assert_eq!(state.time(), 0.0, "a state is entered at its start");
    let fade = state
        .fade()
        .expect("idle -> run fades over a quarter second");
    assert_eq!(fade.from(), asset.initial());
}

/// **A trigger is consumed by the transition that reads it**, and one set
/// during a fade waits for the fade to finish rather than being lost.
#[test]
fn a_trigger_waits_out_a_fade_and_is_consumed_when_it_fires() {
    let machine = character();
    let asset = machine.asset();
    let speed = asset.float_parameter("speed").expect("declared");
    let jump = asset.trigger_parameter("jump").expect("declared");
    let mut state = machine.start();
    let mut events = Vec::new();

    state.set_float(speed, 3.0);
    machine.step(&mut state, TICK, &mut events);
    assert!(state.fade().is_some(), "idle -> run is fading");
    state.trigger(jump);
    // The fade is a quarter second: sixteen steps, the last of which ends it.
    for _ in 0..15 {
        machine.step(&mut state, TICK, &mut events);
        assert_eq!(state.state(), asset.state("run").expect("declared"));
        assert!(state.is_triggered(jump), "the trigger was dropped mid-fade");
    }
    machine.step(&mut state, TICK, &mut events);
    assert_eq!(state.state(), asset.state("jump").expect("declared"));
    assert!(
        !state.is_triggered(jump),
        "the transition did not consume its trigger"
    );
}

/// **An exit time holds a transition back until the state reaches it.** The
/// jump is a half-second one-shot, so thirty-two steps bring it to time 1.
#[test]
fn a_transition_waits_for_its_exit_time() {
    let machine = character();
    let asset = machine.asset();
    let jump = asset.trigger_parameter("jump").expect("declared");
    let jump_state = asset.state("jump").expect("declared");
    let mut state = machine.start();
    let mut events = Vec::new();

    state.trigger(jump);
    machine.step(&mut state, TICK, &mut events);
    assert_eq!(state.state(), jump_state, "idle -> jump is a cut");

    // `grounded` is true throughout, so only the exit time holds it.
    for step in 1..32 {
        machine.step(&mut state, TICK, &mut events);
        assert_eq!(state.state(), jump_state, "it left the jump at step {step}");
    }
    machine.step(&mut state, TICK, &mut events);
    assert_eq!(
        state.state(),
        asset.initial(),
        "the jump did not end at time 1"
    );
}

/// **An exit time on a looping state is reached across a wrap**: a step that
/// carries the time from 0.75 past 0.9 to 0.0 never *stands* at or past the
/// exit time, so only the crossing says it was reached.
#[test]
fn an_exit_time_skipped_over_by_a_wrap_is_still_reached() {
    let machine = single(
        "Clip(\"march\")",
        "",
        "Transition(from: \"loop\", to: \"after\", exit_time: Some(0.9), crossfade: 0.0)",
    );
    let after = machine.asset().state("after").expect("declared");
    let mut state = machine.start();
    let mut events = Vec::new();
    let quarter = 0.25;
    for step in 0..3 {
        machine.step(&mut state, quarter, &mut events);
        assert_ne!(state.state(), after, "it left at step {step}, before 0.9");
    }
    machine.step(&mut state, quarter, &mut events);
    assert_eq!(state.state(), after, "the wrap past 0.9 did not count");
}

/// **The crossfade weight rises evenly over its duration**, and the pose drawn
/// mid-fade is the two states' poses mixed by exactly that weight.
#[test]
fn the_crossfade_weight_progresses_over_its_duration() {
    let machine = character();
    let asset = machine.asset();
    let speed = asset.float_parameter("speed").expect("declared");
    let skeleton = skeleton();
    let mut state = machine.start();
    let mut events = Vec::new();

    state.set_float(speed, 2.0);
    machine.step(&mut state, TICK, &mut events);
    let steps = 16; // the quarter-second fade, in sixty-fourths
    for step in 1..steps {
        machine.step(&mut state, TICK, &mut events);
        let fade = state.fade().expect("still fading");
        assert_eq!(
            fade.weight(),
            step as f32 / steps as f32,
            "the weight after step {step}"
        );
    }

    // Mid-fade, the drawn pose is the blend of the two at that weight.
    let fade = state.fade().expect("still fading");
    let mut drawn = Pose::new(&skeleton);
    Sampler::new(&skeleton, None).sample_into(&machine, &state, &skeleton, &mut drawn);
    let mut outgoing = Pose::new(&skeleton);
    clip("idle")
        .expect("named")
        .sample_into(0.0, &skeleton, &mut outgoing);
    let mut incoming = Pose::new(&skeleton);
    clip("walk")
        .expect("named")
        .sample_into(state.time(), &skeleton, &mut incoming);
    let mut expected = Pose::new(&skeleton);
    blend_into(&outgoing, &incoming, fade.weight(), &mut expected);
    assert_eq!(drawn, expected);

    machine.step(&mut state, TICK, &mut events);
    assert_eq!(state.fade(), None, "the fade outlived its duration");
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Every event a run of `steps` steps of `dt` reports, with the step it
/// landed on.
fn fired(machine: &Machine, dt: f32, steps: u32) -> Vec<(u32, EventId)> {
    let mut state = machine.start();
    let mut events = Vec::new();
    let mut log = Vec::new();
    for step in 0..steps {
        machine.step(&mut state, dt, &mut events);
        log.extend(events.iter().map(|&event| (step, event)));
    }
    log
}

/// **Each crossing fires once — including the ones a loop wrap carries.**
///
/// A one-second clip with footsteps at 0 and 0.5, stepped three
/// sixty-fourths at a time: every third or so step crosses the seam mid-step,
/// and sixty-four steps are exactly three cycles, so exactly six footsteps.
#[test]
fn events_fire_once_per_crossing_across_a_loop_wrap() {
    let machine = single(
        "Clip(\"march\")",
        "Event(at: 0.0, name: \"left\"), Event(at: 0.5, name: \"right\")",
        "",
    );
    let left = machine.asset().event("left").expect("declared");
    let right = machine.asset().event("right").expect("declared");
    let log = fired(&machine, 3.0 / 64.0, 64);
    let count = |id| log.iter().filter(|&&(_, event)| event == id).count();
    assert_eq!(count(left), 3, "left steps: {log:?}");
    assert_eq!(count(right), 3, "right steps: {log:?}");

    // And each on the step that carried the time across it: steps of 3/64
    // reach 0.5 during step 10 (from 30/64 to 33/64) and the seam during
    // step 21 (from 63/64 to 66/64).
    assert_eq!(log[0], (0, left), "time 0 is crossed by the first step");
    assert_eq!(log[1], (10, right));
    assert_eq!(log[2], (21, left), "the wrap's crossing landed elsewhere");
}

/// A step that wraps several whole cycles reports an event once per cycle.
#[test]
fn a_step_spanning_several_cycles_fires_once_per_cycle() {
    let machine = single("Clip(\"march\")", "Event(at: 0.25, name: \"beat\")", "");
    let log = fired(&machine, 3.5, 1);
    assert_eq!(log.len(), 4, "0.0 to 3.5 crosses 0.25 four times: {log:?}");
}

/// **A state's track fires from its entry**, and an outgoing state's track is
/// muted while it fades out.
#[test]
fn entering_a_state_fires_its_start_and_the_outgoing_track_is_muted() {
    let machine = character();
    let asset = machine.asset();
    let speed = asset.float_parameter("speed").expect("declared");
    let footstep = asset.event("footstep").expect("declared");
    let mut state = machine.start();
    let mut events = Vec::new();

    state.set_float(speed, 2.0);
    machine.step(&mut state, TICK, &mut events);
    assert!(events.is_empty(), "idle carries no events");
    machine.step(&mut state, TICK, &mut events);
    assert_eq!(events, [footstep], "the run's first step crosses time 0");

    // Run to just short of the mid-stride footstep, then slow: the run fades
    // out over the next sixteen steps, carrying its time across 0.5 — and that
    // footstep does not fire, because idle is the current state.
    for _ in 0..24 {
        machine.step(&mut state, TICK, &mut events);
    }
    state.set_float(speed, 0.0);
    machine.step(&mut state, TICK, &mut events);
    assert_eq!(state.state(), asset.initial());
    let mut crossed_mid_stride = false;
    for _ in 0..16 {
        let advance = machine.step(&mut state, TICK, &mut events);
        let source = advance.source.expect("the run is fading out");
        crossed_mid_stride |= source.start <= 0.5 && 0.5 < source.end;
        assert!(events.is_empty(), "a faded-out run fired {events:?}");
    }
    assert!(
        crossed_mid_stride,
        "the faded-out run never crossed its footstep, so the mute was not tested"
    );
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

/// The server's tick rate for the cadence property, in hertz.
const SERVER_HZ: f64 = 60.0;

/// How many ticks each cadence run covers: ten seconds of simulation.
const PROPERTY_TICKS: usize = 600;

/// What a run recorded: the state's hash after every tick, and the events each
/// tick fired.
type Record = Vec<(u64, Vec<EventId>)>;

/// Drives [`character`] from frames of the lengths `frames` yields, through a
/// fixed-timestep accumulator at [`SERVER_HZ`], with the parameters each tick
/// drawn from a generator seeded by `seed` — **drawn per tick, not per frame**,
/// because the tick is what a server reads its inputs on.
fn drive(seed: u64, mut frames: impl FnMut() -> f64) -> Record {
    let machine = character();
    let asset = machine.asset();
    let speed = asset.float_parameter("speed").expect("declared");
    let grounded = asset.bool_parameter("grounded").expect("declared");
    let jump = asset.trigger_parameter("jump").expect("declared");
    let tick = 1.0 / SERVER_HZ;

    let mut inputs = crcbl_rand::Rng::from_u64(seed);
    let mut state = machine.start();
    let mut events = Vec::new();
    let mut record = Vec::with_capacity(PROPERTY_TICKS);
    let mut accumulator = 0.0;
    while record.len() < PROPERTY_TICKS {
        accumulator += frames();
        while accumulator >= tick && record.len() < PROPERTY_TICKS {
            accumulator -= tick;
            let draw = inputs.next_u32();
            // Speed held for stretches, so states are held long enough to
            // cross events: a new speed one tick in sixteen.
            if draw.is_multiple_of(16) {
                state.set_float(speed, (draw >> 8) as f32 / (1 << 24) as f32 * 5.0);
            }
            state.set_bool(grounded, draw & (1 << 4) != 0);
            if draw.is_multiple_of(97) {
                state.trigger(jump);
            }
            #[allow(clippy::cast_possible_truncation)]
            machine.step(&mut state, tick as f32, &mut events);
            record.push((hash_of(&state), events.clone()));
        }
    }
    record
}

/// A frame clock of a fixed rate.
fn steady(hz: f64) -> impl FnMut() -> f64 {
    move || 1.0 / hz
}

/// A frame clock whose frames jitter between 2 and 40 ms, from its own seed.
fn jittered(seed: u64) -> impl FnMut() -> f64 {
    let mut frames = crcbl_rand::Rng::from_u64(seed);
    move || 0.002 + f64::from(frames.next_u32() % 38_000) * 1e-6
}

/// **The determinism property.** A seeded parameter sequence gives the same
/// state hash after every tick, and the same events on the same ticks, across
/// two runs and across frame clocks of 30 Hz, 144 Hz and a jittered one — the
/// server's tick count is the clock, and the frame rate is not.
///
/// And the hash is not a constant: a different seed is a different history.
#[test]
fn seeded_parameters_give_the_same_hashes_and_events_whatever_the_frame_rate() {
    for seed in [1, 7, 0x00C0_FFEE] {
        let reference = drive(seed, steady(SERVER_HZ));
        assert_eq!(
            drive(seed, steady(SERVER_HZ)),
            reference,
            "seed {seed}, run twice"
        );
        assert_eq!(
            drive(seed, steady(30.0)),
            reference,
            "seed {seed} at 30 Hz frames"
        );
        assert_eq!(
            drive(seed, steady(144.0)),
            reference,
            "seed {seed} at 144 Hz frames"
        );
        assert_eq!(
            drive(seed, jittered(seed ^ 0x5EED)),
            reference,
            "seed {seed}, jittered"
        );

        let footsteps = reference.iter().flat_map(|(_, events)| events).count();
        assert!(footsteps > 10, "seed {seed} fired only {footsteps} events");
        let states: std::collections::HashSet<u64> = reference.iter().map(|&(h, _)| h).collect();
        assert!(
            states.len() > PROPERTY_TICKS / 2,
            "seed {seed}: the hash barely moved"
        );
    }
    assert_ne!(drive(1, steady(SERVER_HZ)), drive(2, steady(SERVER_HZ)));
}

// ---------------------------------------------------------------------------
// Root motion
// ---------------------------------------------------------------------------

/// **The root-motion velocity is the clip's root delta over `dt`** — on a
/// step inside the cycle and on a step that wraps its seam, where a naive
/// difference of the two sampled positions would point backwards.
#[test]
fn the_root_velocity_is_the_clips_root_delta_over_dt() {
    let machine = single("Clip(\"walk\")", "", "");
    let mut state = machine.start();
    let mut events = Vec::new();
    let expected = Vec3::new(0.0, 0.0, -WALK_STRIDE); // a stride per one-second cycle
    let dt = 3.0 / 64.0;
    let mut wrapped = 0;
    for step in 0..64 {
        let advance = machine.step(&mut state, dt, &mut events);
        if advance.current.end >= 1.0 {
            wrapped += 1;
        }
        let velocity = machine.root_velocity(&advance, &state, 0, dt);
        assert!(
            (velocity - expected).length() < VELOCITY_TOLERANCE,
            "step {step} (span {:?}) measured {velocity:?}",
            advance.current
        );
    }
    assert_eq!(
        wrapped, 3,
        "the run should have crossed the seam three times"
    );
}

/// A clip that does not drive the root authors no motion: in place is zero.
#[test]
fn an_in_place_clip_authors_no_root_motion() {
    let machine = single("Clip(\"march\")", "", "");
    let mut state = machine.start();
    let advance = machine.step(&mut state, TICK, &mut Vec::new());
    assert_eq!(machine.root_velocity(&advance, &state, 0, TICK), Vec3::ZERO);
}

/// **The drawn root stays in place**: with root motion named, the sampled
/// root sits at its rest translation at every point of a clip that walks it a
/// whole stride — and without, the same clip visibly carries it.
#[test]
fn the_rendered_root_stays_in_place() {
    let machine = single("Clip(\"walk\")", "", "");
    let skeleton = skeleton();
    let rest = skeleton.joints()[0].rest.translation;
    let mut stripped = Sampler::new(&skeleton, Some(0));
    let mut carried = Sampler::new(&skeleton, None);
    let mut pose = Pose::new(&skeleton);
    let mut state = machine.start();
    let mut events = Vec::new();
    let mut furthest = 0.0_f32;
    for _ in 0..64 {
        machine.step(&mut state, TICK, &mut events);
        stripped.sample_into(&machine, &state, &skeleton, &mut pose);
        assert_eq!(
            pose.locals()[0].translation,
            rest,
            "at time {}",
            state.time()
        );
        carried.sample_into(&machine, &state, &skeleton, &mut pose);
        furthest = furthest.max((pose.locals()[0].translation - rest).length());
    }
    assert!(
        furthest > 0.5 * WALK_STRIDE,
        "the unstripped root only moved {furthest} m, so the strip proved nothing"
    );
}
