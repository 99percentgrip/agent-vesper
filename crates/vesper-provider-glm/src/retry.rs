use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Frozen retryable GLM HTTP statuses.
pub const RETRYABLE_STATUSES: &[u16] = &[429, 500, 502, 503, 504];

/// Injectable jitter in the closed interval `[0.75, 1.0]`.
pub trait JitterSource: Send + Sync {
    /// Returns the next multiplier.
    fn multiplier(&self, attempt: u32) -> f64;
}

/// Low-state production jitter derived from wall-clock nanoseconds.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemJitter;

impl JitterSource for SystemJitter {
    fn multiplier(&self, attempt: u32) -> f64 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.subsec_nanos());
        let mixed = nanos ^ attempt.wrapping_mul(0x9e37_79b9);
        0.75 + f64::from(mixed % 10_001) / 40_000.0
    }
}

/// Retry policy matching the frozen three-retry/four-attempt behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    /// Number of retries after the initial attempt.
    pub maximum_retries: u32,
    /// Base exponential delay.
    pub base_delay: Duration,
    /// Maximum accepted or computed delay.
    pub maximum_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            maximum_retries: 3,
            base_delay: Duration::from_secs(1),
            maximum_delay: Duration::from_secs(60),
        }
    }
}

impl RetryPolicy {
    /// Whether another attempt is available.
    #[must_use]
    pub const fn permits_retry(self, attempt: u32) -> bool {
        attempt < self.maximum_retries
    }

    /// Whether one HTTP status is retryable.
    #[must_use]
    pub fn status_is_retryable(status: u16) -> bool {
        RETRYABLE_STATUSES.contains(&status)
    }

    /// Resolves Retry-After or capped exponential jitter.
    #[must_use]
    pub fn delay(
        self,
        attempt: u32,
        retry_after: Option<&str>,
        now: SystemTime,
        jitter: &dyn JitterSource,
    ) -> Duration {
        if let Some(value) = retry_after.and_then(|value| parse_retry_after(value, now)) {
            return value.min(self.maximum_delay);
        }
        let exponent = 2_u32.saturating_pow(attempt.min(31));
        let ceiling = self
            .base_delay
            .checked_mul(exponent)
            .unwrap_or(self.maximum_delay)
            .min(self.maximum_delay);
        ceiling.mul_f64(jitter.multiplier(attempt).clamp(0.75, 1.0))
    }
}

/// Parses numeric seconds or an RFC-compatible HTTP date.
#[must_use]
pub fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<f64>()
        && seconds.is_finite()
        && seconds > 0.0
    {
        return Some(Duration::from_secs_f64(seconds.min(60.0)));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    date.duration_since(now)
        .ok()
        .filter(|duration| !duration.is_zero())
        .map(|duration| duration.min(Duration::from_secs(60)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedJitter(f64);
    impl JitterSource for FixedJitter {
        fn multiplier(&self, _attempt: u32) -> f64 {
            self.0
        }
    }

    #[test]
    fn numeric_date_invalid_and_cap_match_source_contract() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(
            parse_retry_after("2.5", now),
            Some(Duration::from_millis(2_500))
        );
        let date = httpdate::fmt_http_date(now + Duration::from_secs(30));
        assert_eq!(parse_retry_after(&date, now), Some(Duration::from_secs(30)));
        let capped = httpdate::fmt_http_date(now + Duration::from_secs(120));
        assert_eq!(
            parse_retry_after(&capped, now),
            Some(Duration::from_secs(60))
        );
        assert_eq!(parse_retry_after("invalid", now), None);
        assert_eq!(
            RetryPolicy::default().delay(1, None, now, &FixedJitter(0.75)),
            Duration::from_millis(1_500)
        );
    }
}
