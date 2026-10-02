//! The decaying histogram of relative arrival delays the playout delay is
//! read from: WebRTC NetEq's `Histogram`
//! (`modules/audio_coding/neteq/histogram.cc`), in `f64` where NetEq keeps
//! Q15 and Q30 fixed point.
//!
//! Each sample is a bucket index. Adding one first scales every bucket by the
//! forget factor and then adds what that took away to the sample's bucket,
//! so the buckets always sum to one and each older sample weighs a forget
//! factor less than the one after it. A new histogram starts with a forget
//! factor of zero and raises it after each sample to
//! `1 − START_FORGET_WEIGHT / (samples + 1)`, until it reaches the base
//! factor: the first samples are weighed close to evenly instead of the
//! first one standing for a whole history. NetEq's fixed point needs a pass
//! that corrects the sum back to one after rounding; `f64` drifts too little
//! to matter.

use std::time::Duration;

use super::{DELAY_FORGET_FACTOR, DELAY_START_FORGET_WEIGHT};

/// How many buckets of `width` cover delays up to `longest`: one more delay
/// lands in the last bucket.
pub(super) const fn bucket_count(width: Duration, longest: Duration) -> usize {
    longest.as_nanos().div_ceil(width.as_nanos()) as usize
}

/// The bucket of `width` that a relative delay of `delay_secs` falls in: the
/// one whose lower edge is the largest multiple of `width` not past it. A
/// delay under zero — an arrival earlier than expected — is in the first, and
/// one past the last bucket in the last.
pub(super) fn bucket_of(delay_secs: f64, width: Duration, buckets: usize) -> usize {
    let index = (delay_secs / width.as_secs_f64()).floor().max(0.0) as usize;
    index.min(buckets - 1)
}

/// A histogram that forgets: see the [module docs](self).
#[derive(Debug, Clone)]
pub(super) struct DelayHistogram {
    buckets: Vec<f64>,
    forget_factor: f64,
    /// How many samples have been added.
    pub(super) samples: u64,
}

impl DelayHistogram {
    pub(super) fn new(buckets: usize) -> Self {
        Self {
            buckets: vec![0.0; buckets],
            forget_factor: 0.0,
            samples: 0,
        }
    }

    /// Add one sample in `bucket`. NetEq's `Histogram::Add`.
    pub(super) fn add(&mut self, bucket: usize) {
        for weight in &mut self.buckets {
            *weight *= self.forget_factor;
        }
        self.buckets[bucket] += 1.0 - self.forget_factor;
        self.samples += 1;
        if self.forget_factor != DELAY_FORGET_FACTOR {
            let ramped = 1.0 - DELAY_START_FORGET_WEIGHT / (self.samples + 1) as f64;
            self.forget_factor = ramped.clamp(0.0, DELAY_FORGET_FACTOR);
        }
    }

    /// The first bucket at which the weight of every bucket after it is no
    /// more than `1 − probability`: the bucket holding the `probability`
    /// quantile. `None` before the first sample. NetEq's
    /// `Histogram::Quantile`.
    pub(super) fn quantile(&self, probability: f64) -> Option<usize> {
        if self.samples == 0 {
            return None;
        }
        let mut above = 1.0;
        for (index, weight) in self.buckets.iter().enumerate() {
            above -= weight;
            if above <= 1.0 - probability {
                return Some(index);
            }
        }
        Some(self.buckets.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playout::DELAY_QUANTILE;

    const WIDTH: Duration = Duration::from_millis(4);

    /// **A delay falls in the bucket whose lower edge is the largest
    /// multiple of the width not past it**; an early arrival in the first and
    /// a delay past the range in the last.
    #[test]
    fn a_delay_lands_in_the_bucket_below_it() {
        assert_eq!(bucket_count(WIDTH, Duration::from_millis(250)), 63);
        assert_eq!(bucket_count(WIDTH, Duration::from_millis(8)), 2);
        assert_eq!(bucket_of(0.0, WIDTH, 10), 0);
        assert_eq!(bucket_of(0.0039, WIDTH, 10), 0);
        assert_eq!(bucket_of(0.004, WIDTH, 10), 1);
        assert_eq!(bucket_of(0.0179, WIDTH, 10), 4);
        assert_eq!(bucket_of(-0.5, WIDTH, 10), 0);
        assert_eq!(bucket_of(1.0, WIDTH, 10), 9);
    }

    /// **The forget factor ramps from zero to [`DELAY_FORGET_FACTOR`]** as
    /// NetEq's start weight sets it, by hand: the first two samples are each
    /// added at a factor of zero (`1 − 2/2`), the third at `1 − 2/3`, the
    /// fourth at `1 − 2/4`.
    #[test]
    fn the_forget_factor_ramps_up_from_zero() {
        assert_eq!(DELAY_START_FORGET_WEIGHT, 2.0);
        let mut histogram = DelayHistogram::new(4);
        histogram.add(0);
        assert_eq!(histogram.buckets, [1.0, 0.0, 0.0, 0.0]);
        histogram.add(1);
        assert_eq!(histogram.buckets, [0.0, 1.0, 0.0, 0.0]);
        histogram.add(2);
        let third = 1.0 - 2.0 / 3.0;
        let expected = [0.0, third, 1.0 - third, 0.0];
        assert_close(&histogram.buckets, &expected);
        histogram.add(3);
        let fourth = 1.0 - 2.0 / 4.0;
        let expected = expected.map(|weight| weight * fourth);
        let expected = [expected[0], expected[1], expected[2], 1.0 - fourth];
        assert_close(&histogram.buckets, &expected);
        assert!((histogram.buckets.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    /// **Past the ramp each sample weighs a forget factor more than the one
    /// before it**: one sample in bucket 1 followed by `n` in bucket 0
    /// leaves bucket 1 its weight times `DELAY_FORGET_FACTOR^n`.
    #[test]
    fn past_the_ramp_old_samples_fade_by_the_forget_factor() {
        let mut histogram = settled_in(0);
        histogram.add(1);
        let added = 1.0 - DELAY_FORGET_FACTOR;
        assert!((histogram.buckets[1] - added).abs() < 1e-12);
        for _ in 0..10 {
            histogram.add(0);
        }
        let faded = added * DELAY_FORGET_FACTOR.powi(10);
        assert!((histogram.buckets[1] - faded).abs() < 1e-12);
        assert!((histogram.buckets[0] - (1.0 - faded)).abs() < 1e-12);
    }

    /// **The quantile is the first bucket with no more than `1 − p` of the
    /// weight after it.** Weights of 1/2, 1/4, 1/8 and 1/8 put the 0.5
    /// quantile in bucket 0, the 0.75 in bucket 1, the 0.875 in bucket 2 and
    /// the 0.9 in bucket 3 — a tail exactly `1 − p` ends the walk, as NetEq's
    /// `sum > inverse_probability` does; an empty histogram has none.
    #[test]
    fn the_quantile_is_the_bucket_the_tail_starts_after() {
        let mut histogram = DelayHistogram::new(4);
        assert_eq!(histogram.quantile(0.95), None);
        histogram.samples = 1;
        histogram.buckets = vec![0.5, 0.25, 0.125, 0.125];
        assert_eq!(histogram.quantile(0.5), Some(0));
        assert_eq!(histogram.quantile(0.75), Some(1));
        assert_eq!(histogram.quantile(0.875), Some(2));
        assert_eq!(histogram.quantile(0.9), Some(3));
    }

    /// **A delay that stops fades out at the forget rate, as a step.** With
    /// every sample in bucket 5 and then every sample in bucket 0, the
    /// quantile stays at 5 while the old weight, `DELAY_FORGET_FACTOR^n`
    /// after `n` new samples, is more than `1 − DELAY_QUANTILE`, and falls
    /// to 0 on the sample that takes it under: the 175th, as
    /// `ln 0.05 / ln 0.983 ≈ 174.7` has it.
    #[test]
    fn a_stopped_delay_holds_then_drops_at_the_forget_rate() {
        assert_eq!((DELAY_QUANTILE, DELAY_FORGET_FACTOR), (0.95, 0.983));
        let held = ((1.0 - DELAY_QUANTILE).ln() / DELAY_FORGET_FACTOR.ln()).ceil() as usize;
        assert_eq!(held, 175);
        let mut histogram = settled_in(5);
        for _ in 1..held {
            histogram.add(0);
            assert_eq!(histogram.quantile(DELAY_QUANTILE), Some(5));
        }
        histogram.add(0);
        assert_eq!(histogram.quantile(DELAY_QUANTILE), Some(0));
    }

    /// A histogram past its ramp with every weight in `bucket`.
    fn settled_in(bucket: usize) -> DelayHistogram {
        let mut histogram = DelayHistogram::new(8);
        while histogram.forget_factor != DELAY_FORGET_FACTOR {
            histogram.add(bucket);
        }
        assert!((histogram.buckets[bucket] - 1.0).abs() < 1e-12);
        histogram
    }

    fn assert_close(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1e-12,
                "{actual:?} against {expected:?}"
            );
        }
    }
}
