// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/backoff.rs - Pure backoff arithmetic (D-01, D-02)
//
// D-02: the wait after the n-th consecutive failure (n counted from zero) is
// `initial * multiplier^n`, capped at `max`. It advances on each consecutive
// transient, rate-limited or auth-required failure and one successful poll
// resets the count. There is no jitter and no gradual decay (both deferred).
// A backoff window can be shorter than the plugin's `poll_interval_secs`; that
// is intended by the locked decision and is not adjusted here.
//
// D-01: on `rate-limited(Some(n))` the wait is the larger of `n` seconds and
// the current step, capped at `max`. With `rate-limited(None)` the wait is the
// step alone.
//
// Both functions are pure: no clock, no IO.
// ─────────────────────────────────────────────────────────────────────────────

use std::time::Duration;

use crate::plugins::config::BackoffConfig;

/// The backoff step after `failures_before` earlier consecutive failures:
/// `initial * multiplier^failures_before`, capped at `cfg.max`. The result is
/// clamped as a float before it becomes a `Duration`, so an infinite power
/// result yields `max` instead of a panic.
#[must_use]
pub fn step(cfg: &BackoffConfig, failures_before: u32) -> Duration {
    let exponent = i32::try_from(failures_before).unwrap_or(i32::MAX);
    let secs = cfg.initial.as_secs_f64() * cfg.multiplier.powi(exponent);
    let max = cfg.max.as_secs_f64();
    // NaN (for example 0 * inf) also falls back to the cap.
    let clamped = if secs.is_nan() { max } else { secs.min(max) };
    Duration::from_secs_f64(clamped.max(0.0))
}

/// The wait after a `rate-limited` failure (D-01): the larger of the plugin's
/// `retry_after` and the current `step`, capped at `cfg.max`.
#[must_use]
pub fn rate_limited_wait(
    retry_after_secs: Option<u32>,
    step: Duration,
    cfg: &BackoffConfig,
) -> Duration {
    let wait = match retry_after_secs {
        None => step,
        Some(n) => Duration::from_secs(u64::from(n)).max(step),
    };
    wait.min(cfg.max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(initial: u64, max: u64, multiplier: f64) -> BackoffConfig {
        BackoffConfig {
            initial: Duration::from_secs(initial),
            max: Duration::from_secs(max),
            multiplier,
        }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn step_sequence_doubles_until_cap() {
        let cfg = cfg(60, 3600, 2.0);
        let got: Vec<Duration> = (0..=8).map(|n| step(&cfg, n)).collect();
        let want: Vec<Duration> = [60, 120, 240, 480, 960, 1920, 3600, 3600, 3600]
            .into_iter()
            .map(secs)
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn step_with_zero_prior_failures_is_initial() {
        assert_eq!(step(&cfg(60, 3600, 2.0), 0), secs(60));
    }

    #[test]
    fn step_never_overflows_for_huge_failure_counts() {
        assert_eq!(step(&cfg(60, 3600, 2.0), u32::MAX), secs(3600));
    }

    #[test]
    fn step_with_multiplier_one_is_constant() {
        let cfg = cfg(60, 3600, 1.0);
        for n in [0, 1, 5, 1000, u32::MAX] {
            assert_eq!(step(&cfg, n), secs(60));
        }
    }

    #[test]
    fn step_is_non_decreasing_and_capped() {
        let cfg = cfg(60, 3600, 2.0);
        let mut previous = Duration::ZERO;
        for n in 0..200 {
            let current = step(&cfg, n);
            assert!(current >= previous, "step {n} went down");
            assert!(current <= cfg.max, "step {n} exceeds max");
            previous = current;
        }
    }

    #[test]
    fn step_exactly_at_cap_stays_at_cap() {
        let cfg = cfg(100, 400, 2.0);
        let got: Vec<Duration> = (0..4).map(|n| step(&cfg, n)).collect();
        assert_eq!(got, vec![secs(100), secs(200), secs(400), secs(400)]);
    }

    #[test]
    fn retry_after_none_uses_step() {
        assert_eq!(rate_limited_wait(None, secs(60), &cfg(60, 3600, 2.0)), secs(60));
    }

    #[test]
    fn retry_after_below_step_uses_step() {
        assert_eq!(
            rate_limited_wait(Some(30), secs(60), &cfg(60, 3600, 2.0)),
            secs(60)
        );
    }

    #[test]
    fn retry_after_equal_to_step_is_that_value() {
        assert_eq!(
            rate_limited_wait(Some(60), secs(60), &cfg(60, 3600, 2.0)),
            secs(60)
        );
    }

    #[test]
    fn retry_after_above_step_is_used() {
        assert_eq!(
            rate_limited_wait(Some(200), secs(60), &cfg(60, 3600, 2.0)),
            secs(200)
        );
    }

    #[test]
    fn retry_after_above_max_is_capped_at_max() {
        assert_eq!(
            rate_limited_wait(Some(100_000), secs(60), &cfg(60, 3600, 2.0)),
            secs(3600)
        );
    }

    #[test]
    fn retry_after_zero_uses_step() {
        assert_eq!(
            rate_limited_wait(Some(0), secs(60), &cfg(60, 3600, 2.0)),
            secs(60)
        );
    }
}
