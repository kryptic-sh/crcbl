//! One peer's input jitter buffer: each input frame held until the tick it
//! targets, and handed to that tick's module in tick order.
//!
//! A client stamps every input with the server tick it is for, and runs its
//! input ahead of the server by about half a round trip plus a margin
//! (`crcbl_client::input_lead`), so a frame normally arrives a tick or two
//! before the server needs it. What the buffer does with one depends on where
//! its tick stands against the tick the server is about to simulate:
//!
//! - **Ahead, within [`MAX_INPUT_LEAD`]:** held, and handed over on its tick.
//! - **The tick about to run:** handed over on it — on time.
//! - **Behind — late:** the tick it targets has already run without it. It is
//!   applied on the tick about to run, the next one there is, and counted:
//!   `21-jobs.md`'s apply-next policy, which the prediction era turns into its
//!   rollback trigger. Dropping it instead would lose an edge (a tap, a
//!   placed tower) for a frame that came a tick too late, and a game reading
//!   its inputs as intent rather than as a per-tick sample wants it late more
//!   than not at all.
//! - **Further ahead than [`MAX_INPUT_LEAD`]:** refused and counted as early,
//!   so a peer stamping ticks far in the future cannot make the server keep
//!   everything it sends.
//!
//! Each tick holds at most [`MAX_CLIENT_INPUTS_PER_TICK`] frames, late ones
//! included; a frame for a full tick is refused and counted against that
//! tick, which is what [`ClientInputs::dropped`](crcbl_ecs::ClientInputs::dropped)
//! reports when the tick runs.
//!
//! Every frame read, whatever became of it, is also a timing sample: the
//! buffer keeps the one that arrived least early since the last snapshot took
//! it ([`InputBuffer::take_timing`]), and the snapshot carries it back to the
//! client ([`InputTiming`]).

use std::collections::BTreeMap;
use std::time::Duration;

use crcbl_core::TickId;
use crcbl_net::{InputTiming, MAX_CLIENT_INPUTS_PER_TICK, MAX_INPUT_LEAD};

/// The ticks [`MAX_INPUT_LEAD`] spans at a tick length of `tick_dt`, rounded
/// up: the furthest ahead of the server's tick a frame is held.
pub(crate) fn horizon_ticks(tick_dt: Duration) -> u64 {
    let ticks = MAX_INPUT_LEAD
        .as_nanos()
        .div_ceil(tick_dt.as_nanos().max(1));
    u64::try_from(ticks).unwrap_or(u64::MAX)
}

/// What became of one frame the buffer was handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrival {
    /// Held for its tick, or the tick about to run.
    InTime,
    /// Its tick had run; it applies on the tick about to.
    Late,
    /// Further ahead than the buffer holds; refused.
    Early,
    /// Its tick already holds [`MAX_CLIENT_INPUTS_PER_TICK`]; refused.
    Full,
}

/// The frames one tick will hand its module, and how many it refused.
#[derive(Debug, Default)]
struct Held {
    /// In tick order, and in arrival order within a tick.
    frames: Vec<(TickId, Vec<u8>)>,
    dropped: u32,
}

/// One peer's frames, held by the tick they target. See the
/// [module docs](self).
#[derive(Debug)]
pub(crate) struct InputBuffer {
    held: BTreeMap<TickId, Held>,
    horizon_ticks: u64,
    /// The frame that arrived least early since the last snapshot took it.
    timing: Option<InputTiming>,
}

impl InputBuffer {
    /// An empty buffer holding frames up to `horizon_ticks` ahead.
    pub(crate) fn new(horizon_ticks: u64) -> Self {
        Self {
            held: BTreeMap::new(),
            horizon_ticks,
            timing: None,
        }
    }

    /// Take one frame for `target`, read while the server is about to
    /// simulate `now`.
    pub(crate) fn receive(&mut self, target: TickId, data: Vec<u8>, now: TickId) -> Arrival {
        let margin = i128::from(target.get()) - i128::from(now.get());
        let margin_ticks =
            i32::try_from(margin).unwrap_or(if margin < 0 { i32::MIN } else { i32::MAX });
        if self
            .timing
            .is_none_or(|timing| margin_ticks < timing.margin_ticks)
        {
            self.timing = Some(InputTiming {
                tick: target,
                margin_ticks,
            });
        }
        if target.get() > now.get().saturating_add(self.horizon_ticks) {
            return Arrival::Early;
        }
        let late = target < now;
        let held = self.held.entry(target.max(now)).or_default();
        if held.frames.len() >= MAX_CLIENT_INPUTS_PER_TICK {
            held.dropped = held.dropped.saturating_add(1);
            return Arrival::Full;
        }
        // After every frame of its tick or earlier: tick order across the
        // late frames a tick takes, arrival order within one tick.
        let at = held.frames.partition_point(|(tick, _)| *tick <= target);
        held.frames.insert(at, (target, data));
        if late { Arrival::Late } else { Arrival::InTime }
    }

    /// Move the frames due by `now` into `frames`, which is emptied first, in
    /// tick order, and return how many of them their ticks refused.
    ///
    /// Only `now`'s are due when the server releases every tick; any earlier
    /// tick still held — one released for no tick — goes with them, ahead of
    /// them, under the same cap.
    pub(crate) fn release(&mut self, now: TickId, frames: &mut Vec<(TickId, Vec<u8>)>) -> u32 {
        frames.clear();
        let later = match now.get().checked_add(1) {
            Some(next) => self.held.split_off(&TickId::from_raw(next)),
            None => BTreeMap::new(),
        };
        let due = std::mem::replace(&mut self.held, later);
        let mut dropped = 0u32;
        for held in due.into_values() {
            dropped = dropped.saturating_add(held.dropped);
            for frame in held.frames {
                if frames.len() >= MAX_CLIENT_INPUTS_PER_TICK {
                    dropped = dropped.saturating_add(1);
                } else {
                    frames.push(frame);
                }
            }
        }
        dropped
    }

    /// The frame that arrived least early since the last call, for the
    /// snapshot about to go to this peer; `None` when none arrived.
    pub(crate) fn take_timing(&mut self) -> Option<InputTiming> {
        self.timing.take()
    }

    /// Forget every frame held and the timing sample: the session they were
    /// sent in is over.
    pub(crate) fn clear(&mut self) {
        self.held.clear();
        self.timing = None;
    }

    /// How many frames are held, over every tick.
    #[cfg(test)]
    pub(crate) fn held_count(&self) -> usize {
        self.held.values().map(|held| held.frames.len()).sum()
    }
}

#[cfg(test)]
mod tests;
