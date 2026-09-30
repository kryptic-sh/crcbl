//! Round-trip time, and the resend timeout it yields.
//!
//! A transcription of RFC 6298, _Computing TCP's Retransmission Timer_,
//! section 2: a smoothed round trip and a mean deviation, updated per sample
//! with gains of 1/8 and 1/4 (Jacobson and Karels), and a timeout of the
//! smoothed value plus four deviations. What differs from TCP is the clamp:
//! RFC 6298's one-second floor is sized for the internet of 2011 and would
//! make a lost command on a LAN cost a second, so the floor and cap here are
//! this layer's own, each argued where it is defined.
//!
//! Samples are unambiguous by construction, which is the problem Karn's
//! algorithm (RFC 6298 section 3) exists to solve for TCP: a resend here goes
//! out in a new packet with a new sequence, so an ack names exactly one
//! transmission and its round trip is never confused with an earlier one's.

use std::time::Duration;

/// The timeout before any sample exists.
///
/// ENet's `ENET_PEER_DEFAULT_ROUND_TRIP_TIME`, as the condition simulator's
/// own first retransmission uses — the closest ecosystem answer to this layer.
/// RFC 6298 says one second; a game's handshake paying a second for one lost
/// packet is the cost that choice would carry.
pub const INITIAL_RTO: Duration = Duration::from_millis(500);

/// The shortest resend timeout, whatever the samples say.
///
/// A LAN round trip is well under a millisecond, and the peer only acks when
/// its loop next runs. Three frames at 60 Hz covers a peer that polls once a
/// frame and was mid-frame when the packet arrived; below that, every
/// scheduling hiccup becomes a spurious resend.
pub const MIN_RTO: Duration = Duration::from_millis(50);

/// The longest resend timeout, after backoff.
///
/// RFC 6298 allows a cap of sixty seconds or more, which suits a stream that
/// may sit idle; a game session is declared dead at
/// [`super::PEER_TIMEOUT`], and a cap well inside that keeps several resends
/// in the window before the verdict.
pub const MAX_RTO: Duration = Duration::from_secs(2);

/// RFC 6298's `G`, the clock granularity. It keeps the deviation term from
/// vanishing when every sample is identical, which a hand-driven clock makes
/// routine.
pub const CLOCK_GRANULARITY: Duration = Duration::from_millis(1);

/// RFC 6298's `K`: deviations of margin above the smoothed round trip.
const DEVIATION_MULTIPLIER: u32 = 4;

/// The estimator: RFC 6298's `SRTT` and `RTTVAR`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RttEstimator {
    smoothed: Option<Duration>,
    variance: Duration,
}

impl RttEstimator {
    /// Fold in one measured round trip (RFC 6298 sections 2.2 and 2.3).
    pub fn sample(&mut self, rtt: Duration) {
        match self.smoothed {
            None => {
                self.smoothed = Some(rtt);
                self.variance = rtt / 2;
            }
            Some(smoothed) => {
                // RTTVAR first, from the SRTT it is about to replace — the
                // order the RFC specifies.
                self.variance = self.variance * 3 / 4 + smoothed.abs_diff(rtt) / 4;
                self.smoothed = Some(smoothed * 7 / 8 + rtt / 8);
            }
        }
    }

    /// The smoothed round trip, once a sample exists.
    #[must_use]
    pub fn smoothed(&self) -> Option<Duration> {
        self.smoothed
    }

    /// The mean deviation of the round trip: the netgraph's jitter.
    #[must_use]
    pub fn variance(&self) -> Duration {
        self.variance
    }

    /// The resend timeout: `SRTT + max(G, K * RTTVAR)` (RFC 6298 section
    /// 2.3), clamped to [`MIN_RTO`]..=[`MAX_RTO`]; [`INITIAL_RTO`] before any
    /// sample.
    #[must_use]
    pub fn rto(&self) -> Duration {
        let Some(smoothed) = self.smoothed else {
            return INITIAL_RTO;
        };
        let margin = CLOCK_GRANULARITY.max(self.variance.saturating_mul(DEVIATION_MULTIPLIER));
        smoothed.saturating_add(margin).clamp(MIN_RTO, MAX_RTO)
    }

    /// The timeout for a fragment already sent `resends` times: the RTO
    /// doubled per resend (RFC 6298 section 5.5), no further than
    /// [`MAX_RTO`].
    #[must_use]
    pub fn backed_off_rto(&self, resends: u32) -> Duration {
        let doubling = 1u32.checked_shl(resends).unwrap_or(u32::MAX);
        self.rto().saturating_mul(doubling).min(MAX_RTO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// RFC 6298 worked by hand: first sample 100 ms gives SRTT 100 and RTTVAR
    /// 50, so RTO 300; a second of 200 ms gives RTTVAR 0.75 * 50 +
    /// 0.25 * 100 = 62.5 and SRTT 0.875 * 100 + 0.125 * 200 = 112.5, so RTO
    /// 112.5 + 4 * 62.5 = 362.5.
    #[test]
    fn the_estimator_matches_rfc_6298_worked_by_hand() {
        let mut rtt = RttEstimator::default();
        assert_eq!(rtt.rto(), INITIAL_RTO);
        assert_eq!(rtt.smoothed(), None);

        rtt.sample(ms(100));
        assert_eq!(rtt.smoothed(), Some(ms(100)));
        assert_eq!(rtt.variance(), ms(50));
        assert_eq!(rtt.rto(), ms(300));

        rtt.sample(ms(200));
        assert_eq!(rtt.variance(), Duration::from_micros(62_500));
        assert_eq!(rtt.smoothed(), Some(Duration::from_micros(112_500)));
        assert_eq!(rtt.rto(), Duration::from_micros(362_500));
    }

    #[test]
    fn the_timeout_is_held_between_its_floor_and_its_cap() {
        let mut fast = RttEstimator::default();
        for _ in 0..64 {
            fast.sample(Duration::from_micros(200));
        }
        assert_eq!(fast.rto(), MIN_RTO);

        let mut slow = RttEstimator::default();
        slow.sample(Duration::from_secs(30));
        assert_eq!(slow.rto(), MAX_RTO);
    }

    #[test]
    fn backoff_doubles_per_resend_up_to_the_cap() {
        let mut rtt = RttEstimator::default();
        rtt.sample(ms(100));
        assert_eq!(rtt.backed_off_rto(0), ms(300));
        assert_eq!(rtt.backed_off_rto(1), ms(600));
        assert_eq!(rtt.backed_off_rto(2), ms(1200));
        assert_eq!(rtt.backed_off_rto(3), MAX_RTO);
        assert_eq!(rtt.backed_off_rto(u32::MAX), MAX_RTO);
    }
}
