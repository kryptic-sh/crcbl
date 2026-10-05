use super::*;

/// A buffer with a horizon long enough for every test that is not about it.
const HORIZON: u64 = 16;

fn tick(raw: u64) -> TickId {
    TickId::from_raw(raw)
}

/// What `buffer` hands the module at `now`: each frame's tick and first
/// byte, and the count refused.
fn release(buffer: &mut InputBuffer, now: u64) -> (Vec<(u64, u8)>, u32) {
    let mut frames = Vec::new();
    let dropped = buffer.release(tick(now), &mut frames);
    let frames = frames
        .into_iter()
        .map(|(tick, data)| (tick.get(), data[0]))
        .collect();
    (frames, dropped)
}

/// **An early frame is held until its tick**: read at tick 10 for tick 13,
/// none of ticks 10 to 12 hands it over, and tick 13 does.
#[test]
fn an_early_frame_is_held_until_its_tick() {
    let mut buffer = InputBuffer::new(HORIZON);
    assert_eq!(buffer.receive(tick(13), vec![7], tick(10)), Arrival::InTime);
    for now in 10..13 {
        assert_eq!(release(&mut buffer, now), (vec![], 0), "tick {now}");
    }
    assert_eq!(release(&mut buffer, 13), (vec![(13, 7)], 0));
    assert_eq!(buffer.held_count(), 0);
}

/// **A late frame applies on the tick about to run, and says it was late**:
/// read at tick 10 for tick 8, it is handed over at tick 10 with its own tick
/// on it, not held for a tick that has gone.
#[test]
fn a_late_frame_applies_on_the_tick_about_to_run() {
    let mut buffer = InputBuffer::new(HORIZON);
    assert_eq!(buffer.receive(tick(8), vec![1], tick(10)), Arrival::Late);
    assert_eq!(buffer.receive(tick(10), vec![2], tick(10)), Arrival::InTime);
    assert_eq!(release(&mut buffer, 10), (vec![(8, 1), (10, 2)], 0));
}

/// **A frame past the horizon is refused, and one on it is held**: the
/// buffer keeps nothing for a tick further ahead than it holds.
#[test]
fn a_frame_past_the_horizon_is_refused() {
    let mut buffer = InputBuffer::new(HORIZON);
    assert_eq!(
        buffer.receive(tick(10 + HORIZON + 1), vec![1], tick(10)),
        Arrival::Early
    );
    assert_eq!(buffer.held_count(), 0);
    assert_eq!(
        buffer.receive(tick(10 + HORIZON), vec![2], tick(10)),
        Arrival::InTime
    );
    assert_eq!(buffer.held_count(), 1);
    // A tick so far ahead its distance overflows is refused too.
    assert_eq!(
        buffer.receive(tick(u64::MAX), vec![3], tick(10)),
        Arrival::Early
    );
}

/// **The order frames arrive in does not decide the order they apply in.**
/// Ticks 12, 11 and 13 read in that order apply one a tick, each on its own;
/// and late frames read newest first apply oldest first, ahead of the tick's
/// own.
#[test]
fn frames_apply_in_tick_order_whatever_order_they_arrive_in() {
    let mut buffer = InputBuffer::new(HORIZON);
    for (target, byte) in [(12, 2), (11, 1), (13, 3)] {
        buffer.receive(tick(target), vec![byte], tick(10));
    }
    assert_eq!(release(&mut buffer, 10), (vec![], 0));
    assert_eq!(release(&mut buffer, 11), (vec![(11, 1)], 0));
    assert_eq!(release(&mut buffer, 12), (vec![(12, 2)], 0));
    assert_eq!(release(&mut buffer, 13), (vec![(13, 3)], 0));

    let mut buffer = InputBuffer::new(HORIZON);
    for (target, byte) in [(20, 0), (19, 9), (17, 7), (18, 8)] {
        buffer.receive(tick(target), vec![byte], tick(20));
    }
    assert_eq!(
        release(&mut buffer, 20),
        (vec![(17, 7), (18, 8), (19, 9), (20, 0)], 0)
    );
}

/// **Each tick holds the per-tick cap, late frames included, and says how
/// many it refused when it runs** — and a frame for the next tick is not
/// refused because this one is full.
#[test]
fn a_tick_holds_the_cap_and_counts_what_it_refused() {
    const EXCESS: usize = 3;
    let mut buffer = InputBuffer::new(HORIZON);
    for i in 0..MAX_CLIENT_INPUTS_PER_TICK + EXCESS {
        // Half of them late, all landing on tick 10.
        let target = if i % 2 == 0 { 10 } else { 9 };
        let arrival = buffer.receive(tick(target), vec![i as u8], tick(10));
        assert_eq!(
            arrival == Arrival::Full,
            i >= MAX_CLIENT_INPUTS_PER_TICK,
            "frame {i}"
        );
    }
    assert_eq!(
        buffer.receive(tick(11), vec![0xFF], tick(10)),
        Arrival::InTime
    );
    let (frames, dropped) = release(&mut buffer, 10);
    assert_eq!(frames.len(), MAX_CLIENT_INPUTS_PER_TICK);
    assert_eq!(dropped, EXCESS as u32);
    assert_eq!(release(&mut buffer, 11), (vec![(11, 0xFF)], 0));
}

/// **A tick that was never released goes with the next release, ahead of
/// it** — nothing held is stranded behind the server's clock.
#[test]
fn a_tick_never_released_goes_with_the_next() {
    let mut buffer = InputBuffer::new(HORIZON);
    buffer.receive(tick(11), vec![1], tick(10));
    buffer.receive(tick(12), vec![2], tick(10));
    assert_eq!(release(&mut buffer, 12), (vec![(11, 1), (12, 2)], 0));
}

/// **The timing sample is the frame that arrived least early, and taking it
/// starts the next one** — late, early and refused frames all count, since
/// each says where the client's lead stands.
#[test]
fn the_timing_sample_is_the_least_early_frame_since_it_was_taken() {
    let mut buffer = InputBuffer::new(HORIZON);
    assert_eq!(buffer.take_timing(), None);
    buffer.receive(tick(13), vec![0], tick(10));
    buffer.receive(tick(11), vec![0], tick(10));
    buffer.receive(tick(12), vec![0], tick(10));
    assert_eq!(
        buffer.take_timing(),
        Some(InputTiming {
            tick: tick(11),
            margin_ticks: 1
        })
    );
    assert_eq!(buffer.take_timing(), None);

    buffer.receive(tick(7), vec![0], tick(10));
    assert_eq!(
        buffer.take_timing(),
        Some(InputTiming {
            tick: tick(7),
            margin_ticks: -3
        })
    );

    buffer.receive(tick(10 + HORIZON + 5), vec![0], tick(10));
    assert_eq!(
        buffer.take_timing().map(|timing| timing.margin_ticks),
        Some(HORIZON as i32 + 5)
    );
}

/// **The horizon is [`MAX_INPUT_LEAD`] in ticks, rounded up**, so a period
/// that does not divide it still holds the whole lead, and one that does
/// holds exactly it.
#[test]
fn the_horizon_is_the_longest_lead_in_ticks_rounded_up() {
    let even = Duration::from_millis(10);
    assert_eq!(
        horizon_ticks(even),
        (MAX_INPUT_LEAD.as_nanos() / even.as_nanos()) as u64
    );
    for uneven in [
        Duration::from_millis(7),
        crcbl_core::FrameClock::new(60).tick_dt(),
    ] {
        let exact = MAX_INPUT_LEAD.as_secs_f64() / uneven.as_secs_f64();
        assert!(exact.fract() > 0.0, "{uneven:?} must not divide the lead");
        assert_eq!(horizon_ticks(uneven) as f64, exact.ceil(), "{uneven:?}");
    }
}
