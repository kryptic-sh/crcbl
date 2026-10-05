use super::*;

/// A tick rate at which a tick is ten milliseconds, so every constant here
/// is a round number of ticks: the step threshold is five, and a feedback
/// window twenty.
const HZ: f64 = 100.0;

/// The server's tick the handshake names, and when its `Accept` arrives.
const ACCEPT_TICK: u64 = 50;
const ACCEPT_AT_MS: u64 = 1_000;
/// The handshake's round trip: four and a half ticks, so the lead it gives —
/// a tick under it, plus [`INPUT_LEAD_MARGIN_TICKS`] — is four.
const ROUND_TRIP_MS: u64 = 45;

/// The server offset the handshake gives: its tick less local time.
const ACCEPT_OFFSET: f64 = ACCEPT_TICK as f64 - ACCEPT_AT_MS as f64 / 10.0;

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// An input clock accepted at [`ACCEPT_TICK`] with [`ROUND_TRIP_MS`].
fn accepted() -> InputLead {
    let mut lead = InputLead::new(HZ);
    lead.accept(
        TickId::from_raw(ACCEPT_TICK),
        ms(ACCEPT_AT_MS),
        ms(ROUND_TRIP_MS),
    );
    lead
}

/// The ticks due at `now_ms`, with the server offset `offset` from a
/// playout (or none, to use the handshake's).
fn advance(lead: &mut InputLead, now_ms: u64, offset: Option<f64>) -> Vec<u64> {
    let mut due = Vec::new();
    lead.advance(ms(now_ms), offset, &mut due);
    due.into_iter().map(TickId::get).collect()
}

/// [`accepted`], advanced once half a tick after the `Accept`: the server
/// time is then half a tick past [`ACCEPT_TICK`], and the clock its lead of
/// four ahead of it.
fn aligned() -> InputLead {
    let mut lead = accepted();
    assert_eq!(advance(&mut lead, ACCEPT_AT_MS + 5, None), [54]);
    lead
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: {actual}, not {expected}"
    );
}

/// **Nothing is due before a handshake**: with no server to follow there
/// is no tick to stamp.
#[test]
fn nothing_is_due_before_the_handshake() {
    let mut lead = InputLead::new(HZ);
    for now in [0, 10, 20] {
        assert!(advance(&mut lead, now, Some(0.0)).is_empty());
    }
    assert_eq!(lead.stats(None).lead, None);
}

/// **The handshake sets the clock a tick under its round trip, plus the
/// margin, ahead of the server time it names.** Half a tick after an
/// `Accept` of tick 50, four and a half ticks of round trip, less one, plus
/// half a tick of margin put the clock at 54.5: tick 54 is
/// due, then one a tick. Setting it is not a step.
#[test]
fn the_handshake_sets_the_clock_a_round_trip_and_a_margin_ahead() {
    let mut lead = aligned();
    assert_close(lead.position.expect("set"), 54.5, "position");
    assert_eq!(advance(&mut lead, ACCEPT_AT_MS + 15, None), [55]);
    assert_eq!(advance(&mut lead, ACCEPT_AT_MS + 25, None), [56]);
    let stats = lead.stats(None);
    assert_eq!(stats.target, Some(ms(40)));
    assert_eq!(stats.steps, 0);
}

/// **An error under the step threshold slews, within the rate bound.** The
/// server time moving two ticks on — twenty milliseconds, under the fifty of
/// [`INPUT_STEP_THRESHOLD`] — is closed by running each tick at most
/// [`MAX_INPUT_RATE_DEVIATION`] fast, never jumping, and the clock arrives.
#[test]
fn a_small_error_slews_within_the_rate_bound() {
    let mut lead = aligned();
    let moved = Some(ACCEPT_OFFSET + 2.0);
    let mut previous = lead.position.expect("set");
    let mut now = ACCEPT_AT_MS + 5;
    let mut faster = 0;
    for _ in 0..200 {
        now += 10;
        let due = advance(&mut lead, now, moved);
        assert!(due.len() <= 2, "a slew sent {due:?} at once");
        let position = lead.position.expect("set");
        let gained = position - previous;
        assert!(
            gained <= 1.0 + MAX_INPUT_RATE_DEVIATION + 1e-9,
            "the clock ran {gained} ticks in one"
        );
        if gained > 1.0 + 1e-9 {
            faster += 1;
        }
        previous = position;
    }
    assert_eq!(lead.stats(moved).steps, 0);
    assert!(faster > 0, "the clock never ran fast");
    // The slew is proportional, so it closes the last of the gap
    // asymptotically: within a hundredth of a tick.
    let target = now as f64 / 10.0 + ACCEPT_OFFSET + 2.0 + 4.0;
    let position = lead.position.expect("set");
    assert!(
        (position - target).abs() < 0.01,
        "the clock slewed to {position}, not {target}"
    );
}

/// **An error past the step threshold steps, and a step forward
/// fast-forwards**: six ticks on, the clock jumps to its target and every
/// tick it passed over is due at once with the current input, so the server
/// holds a frame for each.
#[test]
fn a_large_error_steps_and_fast_forwards_the_ticks_it_passes() {
    let mut lead = aligned();
    let moved = Some(ACCEPT_OFFSET + 6.0);
    assert_eq!(
        advance(&mut lead, ACCEPT_AT_MS + 15, moved),
        (55..=61).collect::<Vec<_>>()
    );
    let stats = lead.stats(moved);
    assert_eq!(stats.steps, 1);
    assert_eq!(stats.skipped_ticks, 0);
    assert_eq!(stats.repeated_ticks, 0);
    assert_close(lead.position.expect("set"), 61.5, "the stepped position");
}

/// **A step forward past the catch-up cap sends only the newest ticks it
/// passed**, and counts the rest as skipped.
#[test]
fn a_step_forward_past_the_catch_up_cap_sends_the_newest_ticks() {
    let mut lead = aligned();
    let moved = Some(ACCEPT_OFFSET + 20.0);
    let due = advance(&mut lead, ACCEPT_AT_MS + 15, moved);
    let catch_up = u64::from(DEFAULT_MAX_CATCH_UP_TICKS);
    let reached = 75;
    assert_eq!(due, (reached + 1 - catch_up..=reached).collect::<Vec<_>>());
    let stats = lead.stats(moved);
    assert_eq!(stats.steps, 1);
    assert_eq!(stats.skipped_ticks, reached - 54 - catch_up);
}

/// **A step back sends no tick twice**: the ticks it passes back over were
/// sent, so nothing is due until the clock passes the newest of them, and
/// how many there were is counted.
#[test]
fn a_step_back_sends_no_tick_twice() {
    let mut lead = aligned();
    let moved = Some(ACCEPT_OFFSET - 6.0);
    assert!(advance(&mut lead, ACCEPT_AT_MS + 15, moved).is_empty());
    let stats = lead.stats(moved);
    assert_eq!(stats.steps, 1);
    // From tick 54, sent, back to 49.5.
    assert_eq!(stats.repeated_ticks, 54 - 49);
    let mut now = ACCEPT_AT_MS + 15;
    for _ in 49..54 {
        now += 10;
        assert!(advance(&mut lead, now, moved).is_empty(), "at {now} ms");
    }
    assert_eq!(advance(&mut lead, now + 10, moved), [55]);
}

/// **A hitch sends at most the catch-up ticks**: three hundred milliseconds
/// in one update reach thirty ticks, and only the newest go.
#[test]
fn a_hitch_sends_at_most_the_catch_up_ticks() {
    let mut lead = aligned();
    let due = advance(&mut lead, ACCEPT_AT_MS + 305, None);
    let catch_up = u64::from(DEFAULT_MAX_CATCH_UP_TICKS);
    assert_eq!(due, (85 - catch_up..85).collect::<Vec<_>>());
    let stats = lead.stats(None);
    assert_eq!(stats.steps, 0);
    assert_eq!(stats.skipped_ticks, 84 - 54 - catch_up);
}

/// Run `lead` on to `until_ms` from `from_ms`, a tick at a time, with the
/// handshake's offset.
fn run(lead: &mut InputLead, from_ms: u64, until_ms: u64) {
    let mut now = from_ms;
    while now < until_ms {
        now += 10;
        advance(lead, now, None);
    }
}

/// The lead the clock steers by, in ticks.
fn lead_ticks(lead: &InputLead) -> f64 {
    lead.lead_ticks.expect("accepted")
}

/// **A late sample raises the lead at once; an early one lowers it by the
/// gain.** Every tick here was sent three and a half ticks ahead of the
/// server time, so one read two ticks late would have been read in time at a
/// lead from five and a half to six and a half, and the lead takes the
/// middle, six, when its window closes; one read five ticks early would have
/// been in time a tick under none, which the lead approaches by
/// [`INPUT_LEAD_GAIN`]. A window closes on the first sample past its end,
/// which opens the next.
#[test]
fn a_late_sample_raises_the_lead_at_once_and_an_early_one_lowers_it_by_the_gain() {
    let mut lead = aligned();
    run(&mut lead, ACCEPT_AT_MS + 5, ACCEPT_AT_MS + 105);
    let window = INPUT_FEEDBACK_WINDOW.as_millis() as u64;

    let late = |tick| InputTiming {
        tick: TickId::from_raw(tick),
        margin_ticks: -2,
    };
    lead.observe(late(56), ms(ACCEPT_AT_MS + 105));
    assert_close(lead_ticks(&lead), 4.0, "a window still open moves nothing");
    run(&mut lead, ACCEPT_AT_MS + 105, ACCEPT_AT_MS + 105 + window);
    lead.observe(late(57), ms(ACCEPT_AT_MS + 105 + window));
    assert_close(
        lead_ticks(&lead),
        3.5 + 2.0 + INPUT_LEAD_MARGIN_TICKS,
        "the late window's need",
    );
    assert_eq!(lead.stats(None).margin_ticks, Some(-2));

    let raised = lead_ticks(&lead);
    let early = |tick| InputTiming {
        tick: TickId::from_raw(tick),
        margin_ticks: 5,
    };
    // Ticks 58 and 59 went before the raise, three and a half ahead. The
    // first closes the window the late tick 57 opened, which asks for the
    // raised lead again; the second closes the window the first opened.
    lead.observe(early(58), ms(ACCEPT_AT_MS + 105 + 2 * window));
    assert_close(lead_ticks(&lead), raised, "the window tick 57 opened");
    lead.observe(early(59), ms(ACCEPT_AT_MS + 105 + 3 * window));
    let needed = 3.5 - 5.0 + INPUT_LEAD_MARGIN_TICKS;
    assert_close(
        lead_ticks(&lead),
        raised + (needed - raised) * INPUT_LEAD_GAIN,
        "the early window's approach",
    );
}

/// **A sample is measured against the lead its own tick was sent with**, so
/// a correction is not counted twice: after one late window raises the lead,
/// more late samples of ticks sent before the raise ask for the same lead,
/// and the lead does not climb again.
#[test]
fn a_sample_is_measured_against_the_lead_its_tick_was_sent_with() {
    let mut lead = aligned();
    run(&mut lead, ACCEPT_AT_MS + 5, ACCEPT_AT_MS + 105);
    let window = INPUT_FEEDBACK_WINDOW.as_millis() as u64;
    let late = |tick| InputTiming {
        tick: TickId::from_raw(tick),
        margin_ticks: -2,
    };
    let mut now = ACCEPT_AT_MS + 105;
    lead.observe(late(56), ms(now));
    now += window;
    lead.observe(late(57), ms(now));
    let raised = lead_ticks(&lead);
    // Each window closes on its first sample, which opens the next; ticks
    // 58 to 61 were all sent before the raise.
    for tick in 58..=61 {
        now += window;
        lead.observe(late(tick), ms(now));
        assert_close(lead_ticks(&lead), raised, "a stale sample's need");
    }
}

/// **A sample of a tick the client does not remember moves nothing** — one
/// from before the last handshake, or older than it keeps — though its
/// margin is still shown.
#[test]
fn a_sample_of_an_unknown_tick_moves_nothing() {
    let mut lead = aligned();
    let window = INPUT_FEEDBACK_WINDOW.as_millis() as u64;
    let unknown = InputTiming {
        tick: TickId::from_raw(3),
        margin_ticks: -40,
    };
    lead.observe(unknown, ms(ACCEPT_AT_MS + 5));
    lead.observe(unknown, ms(ACCEPT_AT_MS + 5 + window));
    assert_close(lead_ticks(&lead), 4.0, "the lead");
    assert_eq!(lead.stats(None).margin_ticks, Some(-40));
}

/// **The lead never passes [`MAX_INPUT_LEAD`]**, past which the server
/// refuses its inputs: a handshake round trip longer than it, and samples
/// asking for more, both stop there.
#[test]
fn the_lead_never_passes_the_longest_the_server_holds() {
    let mut lead = InputLead::new(HZ);
    let longest = MAX_INPUT_LEAD.as_secs_f64() * HZ;
    lead.accept(
        TickId::from_raw(ACCEPT_TICK),
        ms(ACCEPT_AT_MS),
        MAX_INPUT_LEAD * 2,
    );
    assert_close(lead_ticks(&lead), longest, "a long handshake's lead");

    let mut lead = aligned();
    run(&mut lead, ACCEPT_AT_MS + 5, ACCEPT_AT_MS + 105);
    let window = INPUT_FEEDBACK_WINDOW.as_millis() as u64;
    let hopeless = |tick| InputTiming {
        tick: TickId::from_raw(tick),
        margin_ticks: -10_000,
    };
    lead.observe(hopeless(56), ms(ACCEPT_AT_MS + 105));
    lead.observe(hopeless(57), ms(ACCEPT_AT_MS + 105 + window));
    assert_close(lead_ticks(&lead), longest, "a hopeless sample's lead");
}
