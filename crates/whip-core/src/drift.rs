//! Measures the host's effective sample rate against the system clock.
//!
//! Frames arrive in whole host blocks, so comparing two points is off by up
//! to one block. Instead, every observation of the cumulative frame count is
//! fed into an online least-squares fit (Welford's method for covariance);
//! the slope is frames per second of system time.

use std::time::{Duration, Instant};

/// Drift is only reported after this much continuous input.
pub(crate) const MIN_ELAPSED: Duration = Duration::from_secs(10);

pub(crate) struct DriftEstimator {
    origin: Instant,
    frames_origin: u64,
    rate: u32,
    n: f64,
    mean_t: f64,
    mean_f: f64,
    m2_t: f64,
    cov: f64,
    last_t: f64,
}

impl DriftEstimator {
    pub fn new(origin: Instant, frames: u64, rate: u32) -> Self {
        Self {
            origin,
            frames_origin: frames,
            rate,
            n: 0.0,
            mean_t: 0.0,
            mean_f: 0.0,
            m2_t: 0.0,
            cov: 0.0,
            last_t: 0.0,
        }
    }

    /// Records that `frames` (cumulative) had been delivered at `at`.
    pub fn observe(&mut self, at: Instant, frames: u64) {
        let t = at.saturating_duration_since(self.origin).as_secs_f64();
        let f = frames.saturating_sub(self.frames_origin) as f64;
        self.n += 1.0;
        let dt = t - self.mean_t;
        self.mean_t += dt / self.n;
        self.mean_f += (f - self.mean_f) / self.n;
        self.m2_t += dt * (t - self.mean_t);
        self.cov += dt * (f - self.mean_f);
        self.last_t = t;
    }

    /// Deviation from the nominal rate in parts per million.
    pub fn ppm(&self) -> Option<f64> {
        if self.last_t < MIN_ELAPSED.as_secs_f64() || self.m2_t <= 0.0 {
            return None;
        }
        let slope = self.cov / self.m2_t;
        Some((slope / self.rate as f64 - 1.0) * 1e6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates a host delivering `block`-frame callbacks from a clock that
    /// is off by `ppm`, observed at irregular worker ticks.
    fn simulate(rate: u32, block: u64, ppm: f64, seconds: f64) -> f64 {
        let origin = Instant::now();
        let mut d = DriftEstimator::new(origin, 0, rate);
        let block_period = block as f64 / (rate as f64 * (1.0 + ppm * 1e-6));
        let mut tick = 0.0;
        let mut k: u64 = 0;
        while tick < seconds {
            // Worker tick every 5 ms plus a deterministic jitter of up to 3 ms.
            k += 1;
            tick += 0.005 + 0.003 * ((k * 7919 % 13) as f64 / 13.0);
            let delivered = (tick / block_period).floor() as u64 * block;
            d.observe(origin + Duration::from_secs_f64(tick), delivered);
        }
        d.ppm().unwrap()
    }

    #[test]
    fn accurate_without_drift() {
        let ppm = simulate(48_000, 512, 0.0, 30.0);
        assert!(ppm.abs() < 20.0, "measured {ppm} ppm");
    }

    #[test]
    fn resolves_small_drift_with_large_blocks() {
        let ppm = simulate(44_100, 2048, 150.0, 60.0);
        assert!((ppm - 150.0).abs() < 20.0, "measured {ppm} ppm");
        let ppm = simulate(48_000, 256, -80.0, 60.0);
        assert!((ppm + 80.0).abs() < 10.0, "measured {ppm} ppm");
    }

    #[test]
    fn nothing_before_min_elapsed() {
        let origin = Instant::now();
        let mut d = DriftEstimator::new(origin, 0, 48_000);
        d.observe(origin + Duration::from_secs(1), 48_000);
        d.observe(origin + Duration::from_secs(2), 96_000);
        assert!(d.ppm().is_none());
    }
}
