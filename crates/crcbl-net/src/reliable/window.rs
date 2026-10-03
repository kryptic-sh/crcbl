//! What a link did over the last [`STATS_WINDOW`]: bytes each way, packets
//! acknowledged and lost, and resends — the counts a rate or a recent loss
//! figure is read from.
//!
//! The endpoint's own counters run from the link's start, so a rate taken
//! from them is an average over the whole session, which hides a link that
//! went bad a second ago behind the minutes it was fine. A window over them
//! is what the netgraph shows instead.
//!
//! The window is [`STATS_BUCKETS`] buckets of [`STATS_BUCKET`] each, keyed by
//! `now / STATS_BUCKET`, and only **complete** buckets are read: the window at
//! `now` is the buckets before the one `now` falls in. So a reading changes
//! once a bucket, not on every packet, and a rate is always a whole window's
//! count over the window's whole length — never a partly filled bucket scaled
//! up. Each bucket is tagged with its own index, so a bucket left behind by a
//! quiet link is recognised as stale when read and reset when next written,
//! and reading needs no clock tick to roll anything over.

use std::time::Duration;

/// The span a link's rates and recent loss are taken over.
///
/// A second: long enough that a few lost packets at a game's send rate read
/// as a percentage rather than as noise, short enough that a link going bad
/// shows while someone is still looking at the graph.
pub const STATS_WINDOW: Duration = Duration::from_secs(1);

/// The granularity the window moves in: a reading changes this often.
pub const STATS_BUCKET: Duration = Duration::from_millis(100);

/// How many buckets make the window.
pub const STATS_BUCKETS: usize = (STATS_WINDOW.as_nanos() / STATS_BUCKET.as_nanos()) as usize;

const _: () = assert!(STATS_BUCKETS > 0);
const _: () = assert!(STATS_BUCKET.as_nanos() * STATS_BUCKETS as u128 == STATS_WINDOW.as_nanos());

/// The buckets kept: the window's, and the one being filled.
const SLOTS: usize = STATS_BUCKETS + 1;

/// What happened on a link within one bucket, or summed over the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowCounts {
    /// Datagram bytes sent.
    pub bytes_sent: u64,
    /// Datagram bytes received and accepted.
    pub bytes_received: u64,
    /// Sent packets judged acknowledged.
    pub packets_acked: u64,
    /// Sent packets judged lost.
    pub packets_lost: u64,
    /// Reliable fragments sent again after their timeout.
    pub resends: u64,
}

impl WindowCounts {
    fn add(&mut self, other: &Self) {
        self.bytes_sent += other.bytes_sent;
        self.bytes_received += other.bytes_received;
        self.packets_acked += other.packets_acked;
        self.packets_lost += other.packets_lost;
        self.resends += other.resends;
    }

    /// The fraction of the packets judged that were lost, 0 to 1, or `None`
    /// when none was judged.
    #[must_use]
    pub fn loss(&self) -> Option<f32> {
        let judged = self.packets_acked + self.packets_lost;
        (judged > 0).then(|| self.packets_lost as f32 / judged as f32)
    }

    /// [`Self::bytes_sent`] as bytes a second, for counts over
    /// [`STATS_WINDOW`].
    #[must_use]
    pub fn sent_per_second(&self) -> u64 {
        per_second(self.bytes_sent)
    }

    /// [`Self::bytes_received`] as bytes a second, for counts over
    /// [`STATS_WINDOW`].
    #[must_use]
    pub fn received_per_second(&self) -> u64 {
        per_second(self.bytes_received)
    }
}

/// The per-bucket counts. See the module docs.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StatsWindow {
    /// Each slot's counts, and the bucket index they were counted in.
    slots: [(u64, WindowCounts); SLOTS],
}

/// The bucket `now` falls in.
fn bucket_of(now: Duration) -> u64 {
    u64::try_from(now.as_nanos() / STATS_BUCKET.as_nanos()).unwrap_or(u64::MAX)
}

impl StatsWindow {
    /// Count something into the bucket `now` falls in.
    pub(crate) fn count(&mut self, now: Duration, add: impl FnOnce(&mut WindowCounts)) {
        let bucket = bucket_of(now);
        let slot = &mut self.slots[(bucket % SLOTS as u64) as usize];
        if slot.0 != bucket {
            *slot = (bucket, WindowCounts::default());
        }
        add(&mut slot.1);
    }

    /// The counts over the [`STATS_BUCKETS`] complete buckets before the one
    /// `now` falls in.
    pub(crate) fn read(&self, now: Duration) -> WindowCounts {
        let current = bucket_of(now);
        let oldest = current.saturating_sub(STATS_BUCKETS as u64);
        let mut total = WindowCounts::default();
        for (bucket, counts) in &self.slots {
            if (oldest..current).contains(bucket) {
                total.add(counts);
            }
        }
        total
    }
}

/// `count` per [`STATS_WINDOW`], as a per-second rate.
fn per_second(count: u64) -> u64 {
    let window = STATS_WINDOW.as_nanos();
    u64::try_from(u128::from(count) * Duration::from_secs(1).as_nanos() / window)
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(window: &mut StatsWindow, at: Duration, bytes: u64) {
        window.count(at, |counts| counts.bytes_sent += bytes);
    }

    /// Only complete buckets are read: what is counted in the current bucket
    /// shows once that bucket is over, and not before.
    #[test]
    fn the_window_reads_complete_buckets_only() {
        let mut window = StatsWindow::default();
        sent(&mut window, Duration::ZERO, 100);
        sent(&mut window, STATS_BUCKET / 2, 20);
        assert_eq!(window.read(STATS_BUCKET / 2).bytes_sent, 0, "still filling");
        assert_eq!(window.read(STATS_BUCKET).bytes_sent, 120);
        sent(&mut window, STATS_BUCKET, 7);
        assert_eq!(
            window
                .read(STATS_BUCKET * 2 - Duration::from_nanos(1))
                .bytes_sent,
            120,
            "the second bucket is not over a nanosecond before its end"
        );
        assert_eq!(window.read(STATS_BUCKET * 2).bytes_sent, 127);
    }

    /// A bucket leaves the window exactly a window after it closed, and one
    /// written into again after the ring came round holds only its new count.
    #[test]
    fn the_window_rolls_over_one_bucket_at_a_time() {
        let mut window = StatsWindow::default();
        for bucket in 0..STATS_BUCKETS as u32 {
            sent(&mut window, STATS_BUCKET * bucket, 1 << bucket);
        }
        let all = (1u64 << STATS_BUCKETS) - 1;
        assert_eq!(window.read(STATS_WINDOW).bytes_sent, all);
        // The first bucket closed at one bucket in; a window later it is out.
        assert_eq!(
            window.read(STATS_WINDOW + STATS_BUCKET).bytes_sent,
            all - 1,
            "the first bucket has left the window"
        );
        assert_eq!(
            window.read(STATS_WINDOW + STATS_BUCKET * 3).bytes_sent,
            all - 0b111,
        );

        // Written into again a whole ring later, a slot forgets what it held.
        let lap = STATS_BUCKET * SLOTS as u32;
        sent(&mut window, lap, 5);
        assert_eq!(
            window.read(lap + STATS_BUCKET).bytes_sent,
            5 + (all >> 2 << 2),
            "the reused slot carries its new count alone, beside the buckets \
             still inside the window"
        );
    }

    /// A link quiet for longer than a window reads nothing: the stale buckets
    /// are skipped by their tags, with no write needed to clear them.
    #[test]
    fn a_quiet_link_reads_an_empty_window() {
        let mut window = StatsWindow::default();
        sent(&mut window, Duration::ZERO, 50);
        window.count(Duration::ZERO, |counts| counts.packets_lost += 1);
        assert_eq!(window.read(STATS_BUCKET).bytes_sent, 50);
        let later = STATS_WINDOW * 7 + STATS_BUCKET / 3;
        assert_eq!(window.read(later), WindowCounts::default());
        assert_eq!(window.read(later).loss(), None);
    }

    #[test]
    fn loss_is_lost_over_judged() {
        let counts = WindowCounts {
            packets_acked: 15,
            packets_lost: 5,
            ..WindowCounts::default()
        };
        assert_eq!(counts.loss(), Some(0.25));
        assert_eq!(WindowCounts::default().loss(), None);
    }

    /// A window's bytes over its length: half a window's worth of bytes in
    /// a window twice as long would be a quarter of the rate.
    #[test]
    fn a_rate_is_the_windows_bytes_over_its_length() {
        let window_ms = u64::try_from(STATS_WINDOW.as_millis()).unwrap();
        let counts = WindowCounts {
            bytes_sent: 3 * window_ms,
            bytes_received: window_ms,
            ..WindowCounts::default()
        };
        assert_eq!(counts.sent_per_second(), 3_000);
        assert_eq!(counts.received_per_second(), 1_000);
    }
}
