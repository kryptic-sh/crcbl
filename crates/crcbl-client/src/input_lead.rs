//! The input lead: which server tick the client stamps each input with, so
//! that it reaches the server just before the server simulates that tick.
//!
//! The server holds every input until the tick it targets (its jitter buffer,
//! `crcbl_server`'s `input_buffer`) and applies a late one a tick late. An
//! input stamped with the tick the client sees in its snapshots would always
//! be late: those snapshots are a one-way trip old when they arrive, and the
//! input takes another one-way trip to get back. So the client runs its input
//! **ahead** of the server time its snapshots show by a lead — the round trip
//! plus a margin — which puts each input at the server about half a round
//! trip plus the margin ahead of the server's own tick, `21-jobs.md`'s
//! _Tick sync_.
//!
//! # The server time, and the lead
//!
//! The server time the snapshots show is the playout's clock offset (the
//! `playout` module's estimator: arrival time plus the smoothed offset `O`).
//! Before the first snapshot it comes from the handshake: the `Accept` names
//! the server's tick, and its arrival time gives the same offset. The lead
//! starts from the handshake's own round trip, the hello to its `Accept`,
//! less a tick, plus the margin: the hello may wait up to a tick for the
//! server to read it and an input's wait is anywhere in the same tick, so
//! the lead starts low enough that the first inputs err late — still applied,
//! a tick late, and counted — rather than held a tick too long, and the
//! server's samples put it right.
//!
//! **Then the lead follows what the server measured, not the guess.** Each
//! snapshot carries back the input that arrived least early since the last
//! one ([`InputTiming`]): its tick and how many ticks ahead of the server's
//! clock it was read. The client remembers the lead it sent each recent tick
//! with, so one sample says exactly which leads would have had that tick read
//! in the server's last reading before the tick runs — a range a tick wide,
//! starting at the lead it had less the margin it got — and the lead aims
//! [`INPUT_LEAD_MARGIN_TICKS`] into it. The largest of these over each
//! [`INPUT_FEEDBACK_WINDOW`]
//! — the worst arrival in it — sets the lead: a larger one takes over at
//! once, as a later link must be covered from its first late input, and a
//! smaller one is approached by [`INPUT_LEAD_GAIN`] a window. Because each
//! sample is matched to the lead its own tick was sent with, a correction
//! still in flight is never counted twice, however long the round trip.
//!
//! The server reads its transports once a tick, so it measures in whole
//! ticks: what it can say is which reading took an input, not how long before
//! it the input came. The lead therefore aims for the reading just before the
//! tick runs, and sits in the middle of the leads that hit it, half a tick of
//! slack either way; on a jittered link the worst arrival of each window is
//! what places it, so the jitter is covered on top. On a link with no latency
//! at all — a single-player loopback — that is a lead of a fraction of a tick
//! over the server time, and an input reaches the very next tick, as
//! `21-jobs.md` asks of the in-memory transport.
//!
//! # The clock, and the stepped interval
//!
//! The input tick runs at the server's tick rate, sped up or slowed by at most
//! [`MAX_INPUT_RATE_DEVIATION`] in proportion to how far it is from its
//! target, closing the gap over about [`INPUT_CORRECTION_TIME`] — it slews.
//! A slew of even a few percent takes a second to close a gap of a few ticks,
//! and a route change or a resume from suspend opens one of hundreds of
//! milliseconds; so past [`INPUT_STEP_THRESHOLD`] the clock **steps** to its
//! target instead (`21-jobs.md`'s 2026-07-27 correction). What happens to the
//! ticks a step passes over is that correction's "sim-side policy for the
//! stepped interval", and with no client simulation yet it is a policy for
//! the inputs those ticks would have carried:
//!
//! - **A step forward fast-forwards**: every tick it passes over is sent the
//!   current input, as a hitch's ticks are, so the server holds a frame for
//!   each and a game reading held intent sees no gap. At most
//!   [`DEFAULT_MAX_CATCH_UP_TICKS`] go in one update — the frame clock's
//!   spiral guard — and any older than those are dropped and counted
//!   ([`InputLeadStats::skipped_ticks`]).
//! - **A step back repeats nothing**: the ticks it passes back over were sent
//!   already and are held at the server, so no input goes until the clock
//!   passes the newest tick sent; how many ticks that was is counted
//!   ([`InputLeadStats::repeated_ticks`]).
//!
//! Every step is counted, and all three counts reach the netgraph.

use std::collections::VecDeque;
use std::time::Duration;

use crcbl_core::TickId;
use crcbl_core::time::DEFAULT_MAX_CATCH_UP_TICKS;
use crcbl_net::{InputTiming, MAX_INPUT_LEAD};

/// How far into the range of leads that have an input read in time — in the
/// server's last reading before its tick runs — the lead aims, in server
/// ticks: the middle, so a steady input has half a tick of slack before it is
/// read late, and as much before it is read a tick early and held. Jitter is
/// covered on top, by the worst arrival of each [`INPUT_FEEDBACK_WINDOW`].
pub const INPUT_LEAD_MARGIN_TICKS: f64 = 0.5;

/// The furthest the input clock is from its target before it steps there
/// rather than slewing: `21-jobs.md`'s ~50 ms.
pub const INPUT_STEP_THRESHOLD: Duration = Duration::from_millis(50);

/// The most the input clock runs faster or slower than the server's tick rate
/// while it slews to its target, as a fraction of that rate.
///
/// Higher than `21-jobs.md`'s ±0.5%, which was sized for a client whose
/// predicted simulation runs on this clock: with no prediction yet, running
/// the input clock a little fast or slow only changes how far ahead the
/// server holds its inputs, and at ±0.5% a gap just under the step threshold
/// would take many seconds to close while inputs arrive late.
pub const MAX_INPUT_RATE_DEVIATION: f64 = 0.05;

/// The time constant of the input clock's slew: a gap of this long would be
/// closed in this long at the rate it starts at, were the rate not capped by
/// [`MAX_INPUT_RATE_DEVIATION`].
pub const INPUT_CORRECTION_TIME: Duration = Duration::from_millis(250);

/// How long the lead gathers the server's timing samples before it moves on
/// the worst of them. Several snapshots long at every rate the server's rate
/// drop reaches, so the worst arrival in a burst counts rather than the
/// punctual ones behind it.
pub const INPUT_FEEDBACK_WINDOW: Duration = Duration::from_millis(200);

/// How far the lead moves a window towards a smaller lead the samples ask
/// for. A larger one is taken at once.
pub const INPUT_LEAD_GAIN: f64 = 0.25;

/// How long the lead each tick was sent with is remembered, to match the
/// server's timing sample for that tick against: the longest lead a client
/// may run plus as long again for the sample's trip back.
const SENT_LEAD_MEMORY: Duration = MAX_INPUT_LEAD.saturating_mul(2);

/// How the input lead stands: what a netgraph shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputLeadStats {
    /// How far the input tick runs ahead of the server time the snapshots
    /// show — a round trip and a fraction of a tick, when converged on a
    /// steady link; `None` before the handshake.
    pub lead: Option<Duration>,
    /// The lead the input clock is steering towards.
    pub target: Option<Duration>,
    /// The latest timing sample's margin: how many ticks ahead of the
    /// server's clock the server read the input that arrived least early
    /// since the snapshot before — zero is in time, the aim; negative was
    /// late. `None` before the first.
    pub margin_ticks: Option<i32>,
    /// Times the input clock stepped to its target instead of slewing.
    pub steps: u64,
    /// Ticks no input was sent for: past [`DEFAULT_MAX_CATCH_UP_TICKS`] in
    /// one update, after a hitch or a step forward.
    pub skipped_ticks: u64,
    /// Ticks a step back passed back over, sent once already and not again.
    pub repeated_ticks: u64,
}

/// The largest lead the samples of one feedback window asked for, and when
/// the window began, in local ticks.
#[derive(Debug, Clone, Copy)]
struct Window {
    began_at: f64,
    largest: f64,
}

/// The input clock and the lead it steers by. See the [module docs](self).
#[derive(Debug, Clone)]
pub(crate) struct InputLead {
    tick_rate_hz: f64,
    /// The server's tick less local time, both in ticks, from the
    /// handshake's `Accept`: the offset until a snapshot gives one.
    accept_offset: Option<f64>,
    /// How far ahead of the server time the snapshots show the input clock
    /// aims, in ticks; `None` until the handshake.
    lead_ticks: Option<f64>,
    /// Where the input clock is, in server ticks; `None` until it is set.
    position: Option<f64>,
    last_update: Option<Duration>,
    /// The newest tick handed out to send.
    newest_sent: Option<u64>,
    /// The ticks sent over the last [`SENT_LEAD_MEMORY`], oldest first, each
    /// with its lead over the server time when it was sent.
    sent: VecDeque<(u64, f64)>,
    sent_capacity: usize,
    window: Option<Window>,
    last_margin: Option<i32>,
    steps: u64,
    skipped_ticks: u64,
    repeated_ticks: u64,
}

impl InputLead {
    pub(crate) fn new(tick_rate_hz: f64) -> Self {
        let ticks = |duration: Duration| duration.as_secs_f64() * tick_rate_hz;
        Self {
            tick_rate_hz,
            accept_offset: None,
            lead_ticks: None,
            position: None,
            last_update: None,
            newest_sent: None,
            sent: VecDeque::new(),
            sent_capacity: ticks(SENT_LEAD_MEMORY).ceil() as usize,
            window: None,
            last_margin: None,
            steps: 0,
            skipped_ticks: 0,
            repeated_ticks: 0,
        }
    }

    fn ticks(&self, duration: Duration) -> f64 {
        duration.as_secs_f64() * self.tick_rate_hz
    }

    fn duration(&self, ticks: f64) -> Duration {
        Duration::from_secs_f64((ticks / self.tick_rate_hz).max(0.0))
    }

    /// The longest lead the clock runs, in ticks: [`MAX_INPUT_LEAD`], past
    /// which the server refuses its inputs.
    fn max_lead_ticks(&self) -> f64 {
        self.ticks(MAX_INPUT_LEAD)
    }

    /// A handshake was accepted at `now`, naming the server's tick
    /// `server_tick`, `round_trip` after its hello went. The clock starts
    /// over from it: whatever server it was following before, this one is
    /// the one its inputs go to.
    pub(crate) fn accept(&mut self, server_tick: TickId, now: Duration, round_trip: Duration) {
        // Every tick a session reaches is far below 2^53, so `f64` holds it
        // exactly.
        self.accept_offset = Some(server_tick.get() as f64 - self.ticks(now));
        // A tick under the round trip: see the module docs.
        let lead = (self.ticks(round_trip) - 1.0).max(0.0) + INPUT_LEAD_MARGIN_TICKS;
        self.lead_ticks = Some(lead.min(self.max_lead_ticks()));
        self.position = None;
        self.newest_sent = None;
        self.sent.clear();
        self.window = None;
        self.last_margin = None;
    }

    /// Move the input clock on to `now` and push every tick it reached that
    /// is due an input onto `due`, oldest first. `playout_offset` is the
    /// playout's server offset, once a snapshot has given one.
    pub(crate) fn advance(
        &mut self,
        now: Duration,
        playout_offset: Option<f64>,
        due: &mut Vec<TickId>,
    ) {
        let elapsed = self
            .last_update
            .map_or(Duration::ZERO, |previous| now.saturating_sub(previous));
        self.last_update = Some(now);
        let (Some(lead), Some(offset)) = (self.lead_ticks, playout_offset.or(self.accept_offset))
        else {
            return;
        };
        let server_time = self.ticks(now) + offset;
        let target = server_time + lead;
        let position = match self.position {
            None => target,
            Some(position) => {
                let advanced = position + self.ticks(elapsed);
                let error = target - advanced;
                if error.abs() > self.ticks(INPUT_STEP_THRESHOLD) {
                    self.steps += 1;
                    if let Some(newest) = self.newest_sent {
                        let stepped_to = target.max(0.0).floor() as u64;
                        self.repeated_ticks += newest.saturating_sub(stepped_to);
                    }
                    target
                } else {
                    let rate_deviation = (error / self.ticks(INPUT_CORRECTION_TIME))
                        .clamp(-MAX_INPUT_RATE_DEVIATION, MAX_INPUT_RATE_DEVIATION);
                    let elapsed = self.ticks(elapsed);
                    // Never past the target: on a long frame the
                    // proportional correction would overshoot it.
                    advanced + (rate_deviation * elapsed).clamp(-error.abs(), error.abs())
                }
            }
        };
        self.position = Some(position);

        let reached = position.max(0.0).floor() as u64;
        let first = match self.newest_sent {
            None => reached,
            Some(newest) if newest >= reached => return,
            Some(newest) => newest + 1,
        };
        let catch_up = u64::from(DEFAULT_MAX_CATCH_UP_TICKS);
        let first = if reached - first >= catch_up {
            let kept = reached + 1 - catch_up;
            self.skipped_ticks += kept - first;
            kept
        } else {
            first
        };
        for tick in first..=reached {
            due.push(TickId::from_raw(tick));
            if self.sent.len() == self.sent_capacity {
                self.sent.pop_front();
            }
            self.sent.push_back((tick, tick as f64 - server_time));
        }
        self.newest_sent = Some(reached);
    }

    /// Take the server's timing sample, arrived at `now`. See the
    /// [module docs](self).
    pub(crate) fn observe(&mut self, timing: InputTiming, now: Duration) {
        self.last_margin = Some(timing.margin_ticks);
        let Some(lead) = self.lead_ticks else {
            return;
        };
        let Some(&(_, sent_lead)) = self
            .sent
            .iter()
            .find(|(tick, _)| *tick == timing.tick.get())
        else {
            // Older than this client remembers, or from before its last
            // handshake: nothing to measure it against.
            return;
        };
        let needed = sent_lead - f64::from(timing.margin_ticks) + INPUT_LEAD_MARGIN_TICKS;
        let at = self.ticks(now);
        let window = self.ticks(INPUT_FEEDBACK_WINDOW);
        let Some(current) = &mut self.window else {
            self.window = Some(Window {
                began_at: at,
                largest: needed,
            });
            return;
        };
        if at - current.began_at < window {
            current.largest = current.largest.max(needed);
            return;
        }
        let largest = current.largest;
        *current = Window {
            began_at: at,
            largest: needed,
        };
        let next = if largest > lead {
            largest
        } else {
            lead + (largest - lead) * INPUT_LEAD_GAIN
        };
        self.lead_ticks = Some(next.clamp(0.0, self.max_lead_ticks()));
    }

    /// The newest tick handed out to send; `None` before the first.
    pub(crate) fn newest_sent(&self) -> Option<TickId> {
        self.newest_sent.map(TickId::from_raw)
    }

    /// How the lead stands as of the last [`advance`](Self::advance), with
    /// `playout_offset` as [`advance`](Self::advance) takes it.
    pub(crate) fn stats(&self, playout_offset: Option<f64>) -> InputLeadStats {
        let server_time = self
            .last_update
            .zip(playout_offset.or(self.accept_offset))
            .map(|(now, offset)| self.ticks(now) + offset);
        InputLeadStats {
            lead: self
                .position
                .zip(server_time)
                .map(|(position, server_time)| self.duration(position - server_time)),
            target: self.lead_ticks.map(|lead| self.duration(lead)),
            margin_ticks: self.last_margin,
            steps: self.steps,
            skipped_ticks: self.skipped_ticks,
            repeated_ticks: self.repeated_ticks,
        }
    }
}

#[cfg(test)]
mod tests;
