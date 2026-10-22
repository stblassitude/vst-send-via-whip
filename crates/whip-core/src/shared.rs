use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::settings::ChannelMode;

/// State shared between the host's audio thread, the editor and the worker.
///
/// The audio thread only touches atomics. The status snapshot is behind a
/// mutex and is only accessed by the worker and the editor.
pub struct Shared {
    enabled: AtomicBool,
    gain_bits: AtomicU32,
    channel_mode: AtomicU8,
    sample_rate: AtomicU32,
    pub(crate) shutdown: AtomicBool,
    pub(crate) reconnect: AtomicBool,
    pub(crate) input: InputCounters,
    status: Mutex<Status>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            enabled: AtomicBool::new(true),
            gain_bits: AtomicU32::new(1.0f32.to_bits()),
            channel_mode: AtomicU8::new(ChannelMode::MonoLeft as u8),
            sample_rate: AtomicU32::new(48_000),
            shutdown: AtomicBool::new(false),
            reconnect: AtomicBool::new(false),
            input: InputCounters::default(),
            status: Mutex::new(Status::default()),
        })
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Linear gain applied before encoding.
    pub fn set_gain(&self, gain: f32) {
        self.gain_bits.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Relaxed))
    }

    pub fn set_channel_mode(&self, mode: ChannelMode) {
        self.channel_mode.store(mode as u8, Ordering::Relaxed);
    }

    pub fn channel_mode(&self) -> ChannelMode {
        ChannelMode::from_u8(self.channel_mode.load(Ordering::Relaxed))
    }

    pub fn set_sample_rate(&self, rate: u32) {
        self.sample_rate.store(rate, Ordering::Relaxed);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Drop the current session and connect again.
    pub fn request_reconnect(&self) {
        self.reconnect.store(true, Ordering::Relaxed);
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub(crate) fn publish_status(&self, status: &Status) {
        if let Ok(mut s) = self.status.lock() {
            s.clone_from(status);
        }
    }
}

/// Counters updated by the audio thread through [`crate::Feeder`].
#[derive(Default)]
pub(crate) struct InputCounters {
    /// Frames delivered by the host, including those dropped on overrun.
    pub frames: AtomicU64,
    pub blocks: AtomicU64,
    pub overrun_frames: AtomicU64,
    pub last_block: AtomicU32,
    interval_count: AtomicU64,
    interval_sum_us: AtomicU64,
    interval_sumsq_us: AtomicU64,
    interval_min_us: AtomicU64,
    interval_max_us: AtomicU64,
}

impl InputCounters {
    pub fn record_interval(&self, interval: Duration) {
        let us = interval.as_micros().min(u32::MAX as u128) as u64;
        self.interval_count.fetch_add(1, Ordering::Relaxed);
        self.interval_sum_us.fetch_add(us, Ordering::Relaxed);
        self.interval_sumsq_us.fetch_add(us * us, Ordering::Relaxed);
        // A zero minimum means "unset", so store the minimum offset by one.
        let stored = us + 1;
        let _ = self
            .interval_min_us
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |m| {
                (m == 0 || stored < m).then_some(stored)
            });
        self.interval_max_us.fetch_max(us, Ordering::Relaxed);
    }

    /// Returns the interval statistics since the previous call and resets them.
    /// The counters are not read atomically as a group; an occasional
    /// off-by-one block is acceptable for monitoring.
    pub fn take_intervals(&self) -> IntervalStats {
        let count = self.interval_count.swap(0, Ordering::Relaxed);
        let sum = self.interval_sum_us.swap(0, Ordering::Relaxed);
        let sumsq = self.interval_sumsq_us.swap(0, Ordering::Relaxed);
        let min = self.interval_min_us.swap(0, Ordering::Relaxed);
        let max = self.interval_max_us.swap(0, Ordering::Relaxed);
        if count == 0 {
            return IntervalStats::default();
        }
        let n = count as f64;
        let mean = sum as f64 / n;
        let var = (sumsq as f64 / n - mean * mean).max(0.0);
        IntervalStats {
            count,
            min_ms: min.saturating_sub(1) as f64 / 1000.0,
            mean_ms: mean / 1000.0,
            max_ms: max as f64 / 1000.0,
            stddev_ms: var.sqrt() / 1000.0,
        }
    }
}

/// Statistics of the time between consecutive audio callbacks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IntervalStats {
    pub count: u64,
    pub min_ms: f64,
    pub mean_ms: f64,
    pub max_ms: f64,
    pub stddev_ms: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnState {
    /// No usable endpoint configured.
    #[default]
    Unconfigured,
    /// Sending switched off.
    Disabled,
    Connecting,
    Live,
    /// Waiting before the next connection attempt.
    Retrying,
}

impl ConnState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unconfigured => "unconfigured",
            Self::Disabled => "disabled",
            Self::Connecting => "connecting",
            Self::Live => "live",
            Self::Retrying => "retrying",
        }
    }
}

/// A snapshot of the worker's state for display.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub state: ConnState,
    /// Last error or other explanation for the current state.
    pub detail: String,
    pub live_since: Option<Instant>,
    pub reconnects: u64,
    pub sample_rate: u32,
    pub channels: usize,
    /// Host block size of the most recent callback, in frames.
    pub block_frames: u32,
    /// Callback interval statistics over the last metrics window.
    pub intervals: IntervalStats,
    /// Measured host sample rate relative to the nominal rate, against the
    /// system clock, since input last (re)started.
    pub drift_ppm: Option<f64>,
    /// Highest input queue depth over the last metrics window.
    pub queue_max_ms: f64,
    pub overrun_frames: u64,
    pub silence_frames: u64,
    pub trimmed_frames: u64,
    pub frames_encoded: u64,
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub nacks: u64,
    pub rtt_ms: Option<f64>,
    /// Fraction lost as reported by the receiver, 0..1.
    pub loss: Option<f32>,
    pub remote_jitter_ms: Option<f64>,
    pub remote_packets_lost: Option<u64>,
    /// Post-gain peak level, linear, decaying.
    pub peak: f32,
}
