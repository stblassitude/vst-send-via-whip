//! The sender thread: drains audio from the ring buffer, encodes it, and runs
//! the WHIP/WebRTC session. The worker itself never waits on the network:
//! HTTP requests and name lookups run on helper threads, which are left to
//! finish on their own at shutdown, so dropping a [`Sender`] is quick even
//! when the server is unreachable. A panic in the worker ends the current
//! session and the worker carries on.

use std::any::Any;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, RwLock, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rtrb::Consumer;
use str0m::change::{SdpAnswer, SdpPendingOffer};
use str0m::format::{Codec, CodecConfig, CodecSpec, FormatParams, PayloadParams};
use str0m::media::{Direction, Frequency, MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

use crate::drift::DriftEstimator;
use crate::encoder::{OpusFramer, FRAME_SAMPLES};
use crate::eventlog::EventLog;
use crate::resample::To48k;
use crate::settings::{ChannelMode, StreamSettings};
use crate::shared::{ConnState, Shared, Status};
use crate::whip::{self, WhipError, WhipSession};

const TICK: Duration = Duration::from_millis(5);
const FRAME_DURATION: Duration = Duration::from_millis(20);
const POST_TIMEOUT: Duration = Duration::from_secs(5);
const DELETE_TIMEOUT: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Name lookups only decide whether loopback addresses are used; a hanging
/// DNS server must not stall the worker.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(1);
/// How long shutdown waits for a final DELETE before leaving it behind.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);
/// Datagrams handled per socket and tick, so a flood of incoming packets
/// cannot starve the audio path.
const MAX_DATAGRAMS: usize = 256;
/// A live session that receives nothing (STUN consent checks, RTCP) from the
/// server for this long is considered dead. ICE alone takes ~30 s to notice.
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(5);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
/// A session that stayed up this long resets the backoff.
const STABLE_SESSION: Duration = Duration::from_secs(30);
const STATUS_INTERVAL: Duration = Duration::from_millis(200);
/// Input gap after which the worker sends silence on its own clock.
const STALL_MIN: Duration = Duration::from_millis(200);
/// Queue depth above which the oldest audio is discarded, and the depth
/// trimmed down to. Both are raised for large host blocks.
const HIGH_WATER: Duration = Duration::from_millis(150);
const TRIM_TARGET: Duration = Duration::from_millis(40);
const OPUS_PT: u8 = 111;

#[derive(Clone, Debug)]
pub struct SenderOptions {
    /// How often a metrics line is written to the log.
    pub metrics_interval: Duration,
    /// Write to the shared log file in addition to the `log` facade.
    pub log_to_file: bool,
}

impl Default for SenderOptions {
    fn default() -> Self {
        Self {
            metrics_interval: Duration::from_secs(60),
            log_to_file: true,
        }
    }
}

/// Owns the sender thread. Dropping it ends the WHIP session and joins the
/// thread, which takes at most about `RESOLVE_TIMEOUT + SHUTDOWN_GRACE`.
pub struct Sender {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Sender {
    pub fn spawn(
        settings: Arc<RwLock<StreamSettings>>,
        shared: Arc<Shared>,
        consumer: Consumer<f32>,
        options: SenderOptions,
    ) -> std::io::Result<Sender> {
        crate::pin::pin_module();
        shared.shutdown.store(false, Ordering::Relaxed);
        let worker_shared = shared.clone();
        let thread = thread::Builder::new()
            .name("whip-send".into())
            .spawn(move || Worker::new(settings, worker_shared, consumer, options).run())?;
        Ok(Sender {
            shared,
            thread: Some(thread),
        })
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// One WebRTC peer connection and its WHIP resource.
struct Session {
    rtc: Rtc,
    sockets: Vec<UdpSocket>,
    mid: Mid,
    pt: Option<Pt>,
    pending_offer: Option<SdpPendingOffer>,
    post: Option<Receiver<Result<WhipSession, WhipError>>>,
    resource: Option<WhipSession>,
    started: Instant,
    connected: bool,
    last_receive: Instant,
    rtp_time: u64,
    next_timeout: Instant,
}

enum Conn {
    Idle,
    Waiting { until: Instant },
    Active(Box<Session>),
}

/// Channel selection, gain, resampling and framing.
struct Pipeline {
    input_rate: u32,
    mode: ChannelMode,
    resampler: To48k,
    framer: OpusFramer,
    gain: f32,
    raw: Vec<f32>,
    mixed: Vec<f32>,
    at48k: Vec<f32>,
}

impl Pipeline {
    fn new(input_rate: u32, mode: ChannelMode, settings: &StreamSettings) -> Result<Self, String> {
        Ok(Self {
            input_rate,
            mode,
            resampler: To48k::new(input_rate, mode.channels())?,
            framer: OpusFramer::new(mode.channels(), settings).map_err(|e| e.to_string())?,
            gain: 1.0,
            raw: Vec::new(),
            mixed: Vec::new(),
            at48k: Vec::new(),
        })
    }

    /// Maps `raw` (interleaved stereo) to the output channel layout, applying
    /// a gain ramp. Returns the peak level.
    fn mix(&mut self, target_gain: f32) -> f32 {
        let frames = self.raw.len() / 2;
        self.mixed.clear();
        if frames == 0 {
            return 0.0;
        }
        let step = (target_gain - self.gain) / frames as f32;
        let mut g = self.gain;
        let mut peak = 0.0f32;
        for [l, r] in self.raw.as_chunks::<2>().0 {
            g += step;
            let (l, r) = (l * g, r * g);
            match self.mode {
                ChannelMode::MonoLeft => self.mixed.push(l),
                ChannelMode::MonoRight => self.mixed.push(r),
                ChannelMode::MonoSum => self.mixed.push(0.5 * (l + r)),
                ChannelMode::Stereo => {
                    self.mixed.push(l);
                    self.mixed.push(r);
                }
            }
            peak = peak.max(l.abs()).max(r.abs());
        }
        self.gain = target_gain;
        peak
    }
}

struct Worker {
    settings_src: Arc<RwLock<StreamSettings>>,
    settings: StreamSettings,
    shared: Arc<Shared>,
    consumer: Consumer<f32>,
    options: SenderOptions,
    log: EventLog,
    pipeline: Option<Pipeline>,
    conn: Conn,
    backoff: Duration,
    background: Vec<JoinHandle<()>>,
    packets: Vec<Vec<u8>>,
    status: Status,
    last_status: Instant,
    last_metrics: Instant,
    last_input: Instant,
    stalled: bool,
    silence_due: Instant,
    drift: Option<DriftEstimator>,
    trim_logged: Option<Instant>,
}

impl Worker {
    fn new(
        settings_src: Arc<RwLock<StreamSettings>>,
        shared: Arc<Shared>,
        consumer: Consumer<f32>,
        options: SenderOptions,
    ) -> Self {
        let settings = settings_src.read().map(|s| s.clone()).unwrap_or_default();
        let now = Instant::now();
        Self {
            log: EventLog::new(settings.label(), options.log_to_file),
            settings_src,
            settings,
            shared,
            consumer,
            options,
            pipeline: None,
            conn: Conn::Idle,
            backoff: BACKOFF_MIN,
            background: Vec::new(),
            packets: Vec::new(),
            status: Status::default(),
            last_status: now,
            last_metrics: now,
            last_input: now,
            stalled: true,
            silence_due: now,
            drift: None,
            trim_logged: None,
        }
    }

    fn run(mut self) {
        self.log.info(format_args!(
            "sender started, endpoint {}",
            display_endpoint(&self.settings)
        ));
        while !self.shutting_down() {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                while !self.shutting_down() {
                    self.tick();
                }
            }));
            if let Err(payload) = result {
                self.recover(panic_message(&*payload));
            }
        }
        self.teardown("shutting down");
        // Give a final DELETE a moment to reach the server. Anything still
        // running after that is left to finish or time out on its own.
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while self.background.iter().any(|t| !t.is_finished()) && Instant::now() < deadline {
            thread::sleep(TICK);
        }
        self.background.clear();
        self.log.info("sender stopped");
    }

    fn shutting_down(&self) -> bool {
        self.shared.shutdown.load(Ordering::Relaxed)
    }

    fn tick(&mut self) {
        #[cfg(test)]
        if self.shared.inject_panic.swap(false, Ordering::Relaxed) {
            panic!("injected panic");
        }
        let now = Instant::now();
        self.check_settings();
        self.manage_connection(now);
        self.pump_audio(now);
        let next = match &mut self.conn {
            Conn::Active(session) => match service(session, &mut self.status, &self.log, now) {
                Ok(next) => next,
                Err(e) => {
                    self.fail(now, e);
                    now + TICK
                }
            },
            _ => now + TICK,
        };
        self.background.retain(|t| !t.is_finished());
        self.publish(now);
        let wake = next.min(Instant::now() + TICK);
        let sleep = wake.saturating_duration_since(Instant::now());
        if !sleep.is_zero() {
            thread::sleep(sleep);
        }
    }

    /// Resets everything a panic may have left half-done and schedules a
    /// new connection, as for any other failure.
    fn recover(&mut self, msg: &str) {
        self.log.warn(format_args!("internal error: {msg}"));
        self.pipeline = None;
        self.packets.clear();
        self.drift = None;
        self.stalled = true;
        let now = Instant::now();
        self.fail(now, format!("internal error: {msg}"));
        // Don't spin if the panic recurs on every tick.
        let until = now + BACKOFF_MIN;
        while Instant::now() < until && !self.shutting_down() {
            thread::sleep(TICK);
        }
    }

    fn check_settings(&mut self) {
        let current = match self.settings_src.try_read() {
            Ok(current) => current,
            // The settings stay valid if a writer panicked.
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        if *current == self.settings {
            return;
        }
        let new = current.clone();
        drop(current);
        let reconnect = new.connection_differs(&self.settings);
        let reencode = new.bitrate != self.settings.bitrate || new.content != self.settings.content;
        self.settings = new;
        self.log.set_label(self.settings.label());
        self.log.info(format_args!(
            "settings changed: endpoint {}, {} kbit/s, {:?}",
            display_endpoint(&self.settings),
            self.settings.bitrate / 1000,
            self.settings.content
        ));
        if reencode {
            self.pipeline = None;
        }
        if reconnect {
            self.teardown("settings changed");
            self.conn = Conn::Idle;
            self.backoff = BACKOFF_MIN;
        }
    }

    fn manage_connection(&mut self, now: Instant) {
        if self.shared.reconnect.swap(false, Ordering::Relaxed) {
            self.teardown("reconnect requested");
            self.conn = Conn::Idle;
            self.backoff = BACKOFF_MIN;
        }
        let wanted = self.shared.enabled() && self.settings.is_complete();
        if !wanted {
            if !matches!(self.conn, Conn::Idle) {
                self.teardown(if self.shared.enabled() {
                    "endpoint removed"
                } else {
                    "sending disabled"
                });
                self.conn = Conn::Idle;
            }
            self.status.state = if self.settings.is_complete() {
                ConnState::Disabled
            } else {
                ConnState::Unconfigured
            };
            self.status.live_since = None;
            self.status.detail.clear();
            return;
        }
        let start = match self.conn {
            Conn::Idle => true,
            Conn::Waiting { until } => now >= until,
            Conn::Active(_) => false,
        };
        if start {
            match self.connect(now) {
                Ok(session) => {
                    self.conn = Conn::Active(Box::new(session));
                    self.status.state = ConnState::Connecting;
                    self.status.detail = String::new();
                }
                Err(e) => self.fail(now, e),
            }
        }
    }

    fn connect(&mut self, now: Instant) -> Result<Session, String> {
        self.log.info(format_args!(
            "connecting to {}",
            display_endpoint(&self.settings)
        ));
        let sockets = bind_sockets(&self.settings.endpoint)?;

        let mut config = RtcConfig::new()
            .set_crypto_provider(Arc::new(str0m::crypto::from_feature_flags()))
            .set_stats_interval(Some(Duration::from_secs(1)));
        *config.codec_config() = opus_codec_config();
        let mut rtc = config.build(now);
        for s in &sockets {
            let addr = s.local_addr().map_err(|e| e.to_string())?;
            let c = Candidate::host(addr, "udp").map_err(|e| e.to_string())?;
            rtc.add_local_candidate(c);
        }
        let mut api = rtc.sdp_api();
        let mid = api.add_media(
            MediaKind::Audio,
            Direction::SendOnly,
            Some("whip-send".into()),
            Some("audio".into()),
            None,
        );
        let (offer, pending) = api
            .apply()
            .ok_or_else(|| "failed to create SDP offer".to_string())?;
        let offer = offer.to_sdp_string();

        let (tx, rx) = mpsc::channel();
        let endpoint = self.settings.endpoint.clone();
        let token = self.settings.token.clone();
        let log = self.log.clone();
        let t = thread::Builder::new()
            .name("whip-post".into())
            .spawn(move || {
                let result = whip::publish(&endpoint, &token, &offer, POST_TIMEOUT);
                if let Err(mpsc::SendError(Ok(orphan))) = tx.send(result) {
                    // The session was abandoned while the POST was in flight.
                    log.info("ending session created after it was abandoned");
                    let _ = whip::delete(
                        &orphan.resource_url,
                        &token,
                        orphan.etag.as_deref(),
                        DELETE_TIMEOUT,
                    );
                }
            })
            .map_err(|e| e.to_string())?;
        self.background.push(t);

        Ok(Session {
            rtc,
            sockets,
            mid,
            pt: None,
            pending_offer: Some(pending),
            post: Some(rx),
            resource: None,
            started: now,
            connected: false,
            last_receive: now,
            rtp_time: rand_u32() as u64,
            next_timeout: now,
        })
    }

    fn fail(&mut self, now: Instant, reason: String) {
        let was_stable = match &self.conn {
            Conn::Active(s) => s.connected && now.duration_since(s.started) >= STABLE_SESSION,
            _ => false,
        };
        self.teardown(&reason);
        if was_stable {
            self.backoff = BACKOFF_MIN;
        }
        self.log.warn(format_args!(
            "{reason}; retrying in {:.0} s",
            self.backoff.as_secs_f64()
        ));
        self.conn = Conn::Waiting {
            until: now + self.backoff,
        };
        self.status.state = ConnState::Retrying;
        self.status.detail = reason;
        self.status.reconnects += 1;
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
    }

    /// Ends the current session, if any. The DELETE runs in the background
    /// and is joined at shutdown.
    fn teardown(&mut self, reason: &str) {
        let Conn::Active(session) = std::mem::replace(&mut self.conn, Conn::Idle) else {
            return;
        };
        self.status.live_since = None;
        let Some(resource) = session.resource else {
            return;
        };
        self.log.info(format_args!("ending session: {reason}"));
        let token = self.settings.token.clone();
        let log = self.log.clone();
        let t = thread::Builder::new()
            .name("whip-delete".into())
            .spawn(move || {
                if let Err(e) = whip::delete(
                    &resource.resource_url,
                    &token,
                    resource.etag.as_deref(),
                    DELETE_TIMEOUT,
                ) {
                    log.warn(format_args!("ending session failed: {e}"));
                }
            });
        if let Ok(t) = t {
            self.background.push(t);
        }
    }

    /// Moves audio from the ring buffer through the pipeline, and sends
    /// silence when input has stalled.
    fn pump_audio(&mut self, now: Instant) {
        let rate = self.shared.sample_rate().max(1);
        let mode = self.shared.channel_mode();
        let rebuild = match &self.pipeline {
            Some(p) => p.input_rate != rate || p.mode != mode,
            None => true,
        };
        if rebuild {
            match Pipeline::new(rate, mode, &self.settings) {
                Ok(p) => {
                    if self.status.sample_rate != rate {
                        self.drift = None;
                    }
                    self.status.sample_rate = rate;
                    self.status.channels = mode.channels();
                    self.pipeline = Some(p);
                }
                Err(e) => {
                    self.log
                        .warn(format_args!("cannot set up audio pipeline: {e}"));
                    self.pipeline = None;
                }
            }
        }

        let block = self.shared.input.last_block.load(Ordering::Relaxed) as u64;
        let rate64 = rate as u64;
        let block_dur = Duration::from_micros(block * 1_000_000 / rate64);
        let mut available = (self.consumer.slots() / 2) as u64;
        let depth_ms = available as f64 * 1000.0 / rate as f64;
        self.status.queue_max_ms = self.status.queue_max_ms.max(depth_ms);

        let live = matches!(&self.conn, Conn::Active(s) if s.connected && s.pt.is_some());
        let high_water = (HIGH_WATER.as_micros() as u64 * rate64 / 1_000_000).max(2 * block);
        if available > high_water {
            let target = (TRIM_TARGET.as_micros() as u64 * rate64 / 1_000_000).max(block);
            let drop_frames = available - target;
            if let Ok(chunk) = self.consumer.read_chunk(drop_frames as usize * 2) {
                chunk.commit_all();
            }
            available = target;
            self.status.trimmed_frames += drop_frames;
            // Only worth a warning while audio is actually being sent.
            if live
                && self
                    .trim_logged
                    .is_none_or(|t| now.duration_since(t) > Duration::from_secs(10))
            {
                self.log.warn(format_args!(
                    "input queue at {depth_ms:.0} ms, discarded {:.0} ms of the oldest audio",
                    drop_frames as f64 * 1000.0 / rate as f64
                ));
                self.trim_logged = Some(now);
            }
        }

        let Some(pipeline) = self.pipeline.as_mut() else {
            if let Ok(chunk) = self.consumer.read_chunk(available as usize * 2) {
                chunk.commit_all();
            }
            return;
        };

        pipeline.raw.clear();
        if let Ok(chunk) = self.consumer.read_chunk(available as usize * 2) {
            let (a, b) = chunk.as_slices();
            pipeline.raw.extend_from_slice(a);
            pipeline.raw.extend_from_slice(b);
            chunk.commit_all();
        }

        let frames_in = pipeline.raw.len() / 2;
        if frames_in > 0 {
            if self.stalled {
                self.stalled = false;
                self.log.info(format_args!(
                    "input running at {rate} Hz, {block} frames per block"
                ));
                pipeline.framer.clear_pending();
                self.drift = None;
            }
            self.last_input = now;
            // `now` may be stale if this tick also set up a connection.
            let observed = Instant::now();
            let frames = self.shared.input.frames.load(Ordering::Relaxed);
            self.drift
                .get_or_insert_with(|| DriftEstimator::new(observed, frames, rate))
                .observe(observed, frames);
            let peak = pipeline.mix(self.shared.gain());
            self.status.peak = self.status.peak.max(peak);
            if live {
                pipeline.at48k.clear();
                if let Err(e) = pipeline
                    .resampler
                    .process(&pipeline.mixed, &mut pipeline.at48k)
                {
                    self.log.warn(format_args!("resampler: {e}"));
                }
                let packets = &mut self.packets;
                match pipeline
                    .framer
                    .push(&pipeline.at48k, |p| packets.push(p.to_vec()))
                {
                    Ok(n) => self.status.frames_encoded += n as u64,
                    Err(e) => self.log.warn(format_args!("opus: {e}")),
                }
            }
        } else {
            let stall_after = STALL_MIN.max(block_dur * 3);
            if !self.stalled && now.duration_since(self.last_input) > stall_after {
                self.stalled = true;
                self.drift = None;
                self.silence_due = now;
                pipeline.framer.clear_pending();
                self.log.info(format_args!(
                    "no input for {} ms, sending silence",
                    stall_after.as_millis()
                ));
            }
            if self.stalled && live {
                // Paced by the system clock; don't catch up on a long backlog.
                if now.duration_since(self.silence_due) > Duration::from_millis(200) {
                    self.silence_due = now;
                }
                while now >= self.silence_due {
                    let packets = &mut self.packets;
                    if pipeline
                        .framer
                        .silence(|p| packets.push(p.to_vec()))
                        .is_ok()
                    {
                        self.status.silence_frames += 1;
                    }
                    self.silence_due += FRAME_DURATION;
                }
            }
        }

        if let Conn::Active(session) = &mut self.conn {
            if let Some(pt) = session.pt {
                for p in self.packets.drain(..) {
                    let t = MediaTime::new(session.rtp_time, Frequency::FORTY_EIGHT_KHZ);
                    session.rtp_time += FRAME_SAMPLES as u64;
                    if let Some(w) = session.rtc.writer(session.mid) {
                        if let Err(e) = w.write(pt, now, t, p) {
                            self.log.warn(format_args!("write: {e}"));
                        }
                    }
                }
            }
        }
        self.packets.clear();
    }

    fn publish(&mut self, now: Instant) {
        let metrics_due = now.duration_since(self.last_metrics) >= self.options.metrics_interval;
        if !metrics_due && now.duration_since(self.last_status) < STATUS_INTERVAL {
            return;
        }
        let input = &self.shared.input;
        self.status.block_frames = input.last_block.load(Ordering::Relaxed);
        self.status.overrun_frames = input.overrun_frames.load(Ordering::Relaxed);
        self.status.drift_ppm = self.drift.as_ref().and_then(DriftEstimator::ppm);
        if metrics_due {
            self.status.intervals = input.take_intervals();
            self.log_metrics();
            self.status.queue_max_ms = 0.0;
            self.last_metrics = now;
        }
        self.shared.publish_status(&self.status);
        // Decay the peak meter by roughly 20 dB per second.
        let dt = now.duration_since(self.last_status).as_secs_f32();
        self.status.peak *= 0.1f32.powf(dt);
        self.last_status = now;
    }

    fn log_metrics(&self) {
        let s = &self.status;
        let i = &s.intervals;
        let opt = |v: Option<f64>, prec: usize| match v {
            Some(v) => format!("{v:.prec$}"),
            None => "-".into(),
        };
        self.log.info(format_args!(
            "metrics state={} rate={} ch={} block={} cb_ms(n={} min={:.2} avg={:.2} max={:.2} sd={:.2}) \
             drift_ppm={} queue_max_ms={:.1} overrun_frames={} silence_frames={} trimmed_frames={} \
             encoded={} pkts={} bytes={} nacks={} rtt_ms={} loss={} jitter_ms={} lost={} reconnects={}",
            s.state.as_str(),
            s.sample_rate,
            s.channels,
            s.block_frames,
            i.count,
            i.min_ms,
            i.mean_ms,
            i.max_ms,
            i.stddev_ms,
            opt(s.drift_ppm, 1),
            s.queue_max_ms,
            s.overrun_frames,
            s.silence_frames,
            s.trimmed_frames,
            s.frames_encoded,
            s.packets_sent,
            s.bytes_sent,
            s.nacks,
            opt(s.rtt_ms, 1),
            opt(s.loss.map(f64::from), 3),
            opt(s.remote_jitter_ms, 1),
            s.remote_packets_lost
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into()),
            s.reconnects,
        ));
    }
}

/// Drives one session: WHIP answer, UDP input, str0m output and events.
/// Returns when str0m next needs attention.
fn service(
    session: &mut Session,
    status: &mut Status,
    log: &EventLog,
    now: Instant,
) -> Result<Instant, String> {
    if let Some(rx) = &session.post {
        match rx.try_recv() {
            Ok(Ok(ws)) => {
                session.post = None;
                if !ws.ice_servers.is_empty() {
                    log.info(format_args!(
                        "server offered {} ICE server(s); STUN/TURN is not used yet",
                        ws.ice_servers.len()
                    ));
                }
                let answer = SdpAnswer::from_sdp_string(&ws.answer)
                    .map_err(|e| format!("invalid SDP answer: {e}"))?;
                let pending = session.pending_offer.take().ok_or("no pending offer")?;
                session
                    .rtc
                    .sdp_api()
                    .accept_answer(pending, answer)
                    .map_err(|e| format!("SDP answer rejected: {e}"))?;
                session.resource = Some(ws);
            }
            Ok(Err(e)) => {
                session.post = None;
                return Err(format!("WHIP request failed: {e}"));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Err("WHIP request aborted".into()),
        }
    }
    if !session.connected && now.duration_since(session.started) > CONNECT_TIMEOUT {
        return Err("timed out establishing the connection".into());
    }

    if session.connected && now.saturating_duration_since(session.last_receive) > RECEIVE_TIMEOUT {
        return Err(format!(
            "connection lost (nothing received from the server for {} s)",
            RECEIVE_TIMEOUT.as_secs()
        ));
    }

    let mut buf = [0u8; 2048];
    for socket in &session.sockets {
        let Ok(local) = socket.local_addr() else {
            continue;
        };
        for _ in 0..MAX_DATAGRAMS {
            match socket.recv_from(&mut buf) {
                Ok((n, source)) => {
                    let Ok(contents) = (&buf[..n]).try_into() else {
                        continue;
                    };
                    let input = Input::Receive(
                        Instant::now(),
                        Receive {
                            proto: Protocol::Udp,
                            source,
                            destination: local,
                            contents,
                        },
                    );
                    if session.rtc.accepts(&input) {
                        session.last_receive = Instant::now();
                        session.rtc.handle_input(input).map_err(|e| e.to_string())?;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                // ICMP errors surface as ConnectionReset on some platforms.
                Err(e) if e.kind() == ErrorKind::ConnectionReset => continue,
                Err(e) => return Err(format!("UDP receive: {e}")),
            }
        }
    }

    for _ in 0..1000 {
        match session.rtc.poll_output().map_err(|e| e.to_string())? {
            Output::Timeout(t) => {
                let now = Instant::now();
                if t <= now {
                    session
                        .rtc
                        .handle_input(Input::Timeout(now))
                        .map_err(|e| e.to_string())?;
                    continue;
                }
                session.next_timeout = t;
                return Ok(t);
            }
            Output::Transmit(t) => {
                let socket = session
                    .sockets
                    .iter()
                    .find(|s| s.local_addr().map(|a| a == t.source).unwrap_or(false))
                    .or_else(|| session.sockets.first());
                if let Some(socket) = socket {
                    // Send errors (e.g. no route) are transient for ICE.
                    let _ = socket.send_to(&t.contents, t.destination);
                }
            }
            Output::Event(e) => handle_event(session, status, log, e)?,
        }
    }
    Ok(now + TICK)
}

fn handle_event(
    session: &mut Session,
    status: &mut Status,
    log: &EventLog,
    event: Event,
) -> Result<(), String> {
    match event {
        Event::Connected => {
            session.connected = true;
            session.last_receive = Instant::now();
            session.pt = session.rtc.writer(session.mid).and_then(|w| {
                w.payload_params()
                    .find(|p| p.spec().codec == Codec::Opus)
                    .map(|p| p.pt())
            });
            if session.pt.is_none() {
                return Err("server did not accept Opus".into());
            }
            status.state = ConnState::Live;
            status.detail = String::new();
            status.live_since = Some(Instant::now());
            log.info("connected, sending audio");
        }
        Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
            return Err("connection lost (ICE disconnected)".into());
        }
        Event::MediaEgressStats(s) => {
            status.packets_sent = s.packets;
            status.bytes_sent = s.bytes;
            status.nacks = s.nacks;
            if let Some(rtt) = s.rtt {
                status.rtt_ms = Some(rtt.as_secs_f64() * 1000.0);
            }
            if s.loss.is_some() {
                status.loss = s.loss;
            }
            if let Some(remote) = s.remote {
                status.remote_jitter_ms = Some(remote.jitter as f64 / 48.0);
                status.remote_packets_lost = Some(remote.packets_lost);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Opus only, stereo-capable, in-band FEC, NACK offered.
fn opus_codec_config() -> CodecConfig {
    let spec = CodecSpec {
        codec: Codec::Opus,
        clock_rate: Frequency::FORTY_EIGHT_KHZ,
        channels: Some(2),
        format: FormatParams {
            min_p_time: Some(10),
            use_inband_fec: Some(true),
            stereo: Some(true),
            sprop_stereo: Some(true),
            ..Default::default()
        },
    };
    let mut params = PayloadParams::new(Pt::from(OPUS_PT), None, spec);
    params.set_fb_nack(true);
    CodecConfig::new_from_payload_params(vec![params])
}

/// Binds one non-blocking UDP socket per usable local address. Loopback is
/// only used when the endpoint itself is on this machine.
fn bind_sockets(endpoint: &str) -> Result<Vec<UdpSocket>, String> {
    let endpoint_ips = endpoint_addrs(endpoint);
    let local_endpoint = !endpoint_ips.is_empty() && endpoint_ips.iter().all(|ip| ip.is_loopback());
    let ifaces = if_addrs::get_if_addrs().map_err(|e| format!("listing interfaces: {e}"))?;
    let mut ips: Vec<IpAddr> = ifaces
        .iter()
        .map(|i| i.ip())
        .filter(|ip| usable_ip(ip, local_endpoint))
        .collect();
    ips.sort();
    ips.dedup();
    if ips.is_empty() && local_endpoint {
        ips.push(IpAddr::from([127, 0, 0, 1]));
    }
    let sockets: Vec<UdpSocket> = ips
        .into_iter()
        .filter_map(|ip| {
            let s = UdpSocket::bind(SocketAddr::new(ip, 0)).ok()?;
            s.set_nonblocking(true).ok()?;
            Some(s)
        })
        .collect();
    if sockets.is_empty() {
        return Err("no usable network interface".into());
    }
    Ok(sockets)
}

fn usable_ip(ip: &IpAddr, allow_loopback: bool) -> bool {
    if ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    if ip.is_loopback() {
        return allow_loopback;
    }
    match ip {
        IpAddr::V4(v4) => !v4.is_link_local(),
        // Skip link-local (fe80::/10); it needs a scope id to be useful.
        IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) != 0xfe80,
    }
}

fn endpoint_addrs(endpoint: &str) -> Vec<IpAddr> {
    let Some((scheme, rest)) = endpoint.trim().split_once("://") else {
        return Vec::new();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority
        .rsplit_once('@')
        .map(|(_, a)| a)
        .unwrap_or(authority);
    let default_port = if scheme.eq_ignore_ascii_case("https") {
        443
    } else {
        80
    };
    let host_port = if authority.ends_with(']') || !authority.contains(':') {
        format!("{authority}:{default_port}")
    } else {
        authority.to_string()
    };
    if let Ok(addr) = host_port.parse::<SocketAddr>() {
        return vec![addr.ip()];
    }
    // A lookup that outlives the timeout finishes on its own; an endpoint
    // that cannot be resolved quickly is treated as remote.
    let (tx, rx) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("whip-resolve".into())
        .spawn(move || {
            let addrs: Vec<IpAddr> = host_port
                .to_socket_addrs()
                .map(|it| it.map(|a| a.ip()).collect())
                .unwrap_or_default();
            let _ = tx.send(addrs);
        });
    if spawned.is_err() {
        return Vec::new();
    }
    rx.recv_timeout(RESOLVE_TIMEOUT).unwrap_or_default()
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("panic")
}

fn display_endpoint(settings: &StreamSettings) -> String {
    let e = settings.endpoint.trim();
    if e.is_empty() {
        "(none)".into()
    } else if settings.token.trim().is_empty() {
        format!("{e} (no token)")
    } else {
        format!("{e} (with token)")
    }
}

/// A random 32-bit value for the initial RTP timestamp, without pulling in
/// a dependency.
fn rand_u32() -> u32 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    h.write_u128(Instant::now().elapsed().as_nanos());
    h.finish() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_local_and_loopback_are_filtered() {
        assert!(!usable_ip(&"169.254.1.1".parse().unwrap(), false));
        assert!(!usable_ip(&"fe80::1".parse().unwrap(), false));
        assert!(!usable_ip(&"127.0.0.1".parse().unwrap(), false));
        assert!(usable_ip(&"127.0.0.1".parse().unwrap(), true));
        assert!(usable_ip(&"192.168.1.10".parse().unwrap(), false));
        assert!(usable_ip(&"2001:db8::1".parse().unwrap(), false));
    }

    #[test]
    fn endpoint_host_resolution() {
        assert_eq!(
            endpoint_addrs("http://127.0.0.1:8443/group/a/.whip"),
            vec![IpAddr::from([127, 0, 0, 1])]
        );
        assert_eq!(
            endpoint_addrs("https://[::1]/group/a/.whip"),
            vec!["::1".parse::<IpAddr>().unwrap()]
        );
        assert!(endpoint_addrs("garbage").is_empty());
        let local = endpoint_addrs("http://localhost:8444/group/a/.whip");
        assert!(!local.is_empty() && local.iter().all(IpAddr::is_loopback));
    }

    fn spawn_test_sender(settings: StreamSettings) -> (Sender, Arc<Shared>) {
        let shared = Shared::new();
        let (_feeder, consumer) = crate::audio_channel(shared.clone());
        let options = SenderOptions {
            metrics_interval: Duration::from_secs(3600),
            log_to_file: false,
        };
        let settings = Arc::new(RwLock::new(settings));
        let sender = Sender::spawn(settings, shared.clone(), consumer, options).unwrap();
        (sender, shared)
    }

    fn wait_for(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        cond()
    }

    #[test]
    fn worker_survives_panics() {
        let (sender, shared) = spawn_test_sender(StreamSettings::default());
        for n in 1..=2 {
            shared.inject_panic.store(true, Ordering::Relaxed);
            assert!(wait_for(Duration::from_secs(5), || shared
                .status()
                .reconnects
                == n));
        }
        // Still running: the next ticks report the actual state again.
        assert!(wait_for(Duration::from_secs(5), || {
            shared.status().state == ConnState::Unconfigured
        }));
        drop(sender);
    }

    #[test]
    fn shutdown_does_not_wait_for_a_hanging_server() {
        // Connections complete in the backlog, but nothing is ever answered.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let settings = StreamSettings {
            endpoint: format!("http://{}/group/a/.whip", listener.local_addr().unwrap()),
            ..Default::default()
        };
        let (sender, shared) = spawn_test_sender(settings);
        assert!(wait_for(Duration::from_secs(5), || {
            shared.status().state == ConnState::Connecting
        }));
        let start = Instant::now();
        drop(sender);
        let took = start.elapsed();
        assert!(took < Duration::from_secs(2), "shutdown took {took:?}");
    }

    #[test]
    fn codec_config_offers_opus_only() {
        let c = opus_codec_config();
        assert_eq!(c.params().len(), 1);
        assert_eq!(c.params()[0].spec().codec, Codec::Opus);
    }
}
