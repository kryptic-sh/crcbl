//! Deterministic per-client inbound traffic limiting.

use std::time::Duration;

/// Configures the maximum inbound traffic a single server client may send.
///
/// Each limit is a token bucket with capacity equal to one second of traffic.
/// This admits normal short bursts while bounding an idle client's later burst.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundRateLimitConfig {
    /// Maximum accepted inbound messages per second.
    pub messages_per_second: u64,
    /// Maximum accepted inbound payload bytes per second.
    pub bytes_per_second: u64,
}

impl Default for InboundRateLimitConfig {
    fn default() -> Self {
        Self {
            messages_per_second: 120,
            bytes_per_second: 128 * 1024,
        }
    }
}

#[derive(Debug)]
pub(crate) struct InboundRateLimiter {
    config: InboundRateLimitConfig,
    message_tokens: u64,
    byte_tokens: u64,
    message_remainder: u128,
    byte_remainder: u128,
    last_refill: Duration,
}

impl InboundRateLimiter {
    const NANOS_PER_SECOND: u128 = 1_000_000_000;

    pub(crate) fn new(config: InboundRateLimitConfig, now: Duration) -> Self {
        Self {
            message_tokens: config.messages_per_second,
            byte_tokens: config.bytes_per_second,
            config,
            message_remainder: 0,
            byte_remainder: 0,
            last_refill: now,
        }
    }

    pub(crate) fn reconfigure(&mut self, config: InboundRateLimitConfig, now: Duration) {
        *self = Self::new(config, now);
    }

    fn refill(&mut self, now: Duration) {
        let Some(elapsed) = now.checked_sub(self.last_refill) else {
            self.last_refill = now;
            self.message_remainder = 0;
            self.byte_remainder = 0;
            return;
        };
        if elapsed.is_zero() {
            return;
        }
        self.last_refill = now;
        Self::refill_bucket(
            &mut self.message_tokens,
            self.config.messages_per_second,
            &mut self.message_remainder,
            elapsed,
        );
        Self::refill_bucket(
            &mut self.byte_tokens,
            self.config.bytes_per_second,
            &mut self.byte_remainder,
            elapsed,
        );
    }

    fn refill_bucket(tokens: &mut u64, capacity: u64, remainder: &mut u128, elapsed: Duration) {
        let accrued = (capacity as u128)
            .saturating_mul(elapsed.as_nanos())
            .saturating_add(*remainder);
        let replenished = accrued / Self::NANOS_PER_SECOND;
        *remainder = accrued % Self::NANOS_PER_SECOND;
        *tokens = tokens.saturating_add(replenished.min(u64::MAX as u128) as u64);
        if *tokens >= capacity {
            *tokens = capacity;
            *remainder = 0;
        }
    }

    pub(crate) fn allow(&mut self, now: Duration, bytes: u64) -> Result<(), (bool, bool)> {
        self.refill(now);
        let messages_limited = self.message_tokens == 0;
        let bytes_limited = bytes > self.byte_tokens;

        // Charge every inspected packet against both dimensions independently,
        // including packets rejected by the other dimension. Otherwise an
        // oversized-packet stream can bypass the message budget indefinitely.
        self.message_tokens = self.message_tokens.saturating_sub(1);
        self.byte_tokens = self.byte_tokens.saturating_sub(bytes);
        if messages_limited || bytes_limited {
            return Err((messages_limited, bytes_limited));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_and_backward_time_do_not_refill() {
        let mut limiter = InboundRateLimiter::new(
            InboundRateLimitConfig {
                messages_per_second: 1,
                bytes_per_second: 1,
            },
            Duration::from_secs(1),
        );

        assert!(limiter.allow(Duration::from_secs(1), 1).is_ok());
        assert!(limiter.allow(Duration::from_secs(1), 1).is_err());
        assert!(limiter.allow(Duration::ZERO, 1).is_err());
        assert!(limiter.allow(Duration::from_secs(1), 1).is_ok());
    }

    #[test]
    fn byte_rejection_still_consumes_message_budget() {
        let mut limiter = InboundRateLimiter::new(
            InboundRateLimitConfig {
                messages_per_second: 1,
                bytes_per_second: 1,
            },
            Duration::ZERO,
        );

        assert_eq!(limiter.allow(Duration::ZERO, 2), Err((false, true)));
        assert_eq!(limiter.allow(Duration::ZERO, 0), Err((true, false)));
    }
}
