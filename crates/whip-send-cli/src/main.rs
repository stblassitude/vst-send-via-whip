//! Test sender: simulates a plugin host feeding a sine tone into whip-core.
//!
//! The simulated host delivers fixed-size blocks paced by the system clock,
//! optionally running fast or slow by a given amount (to mimic a sound card
//! clock such as Dante Virtual Soundcard), and optionally stalling.

use std::f32::consts::TAU;
use std::process::ExitCode;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use whip_core::{
    audio_channel, ChannelMode, Content, Sender, SenderOptions, Shared, StreamSettings,
};

const USAGE: &str = "\
usage: whip-send-cli --endpoint URL [options]

  --endpoint URL        WHIP endpoint, e.g. http://host:8443/group/test/.whip
  --token TOKEN         bearer token (default: none; also read from WHIP_TOKEN)
  --seconds N           run time, 0 = until killed (default 30)
  --rate HZ             simulated host sample rate (default 48000)
  --block FRAMES        simulated host block size (default 512)
  --drift-ppm PPM       simulated host clock error, e.g. 150 or -80 (default 0)
  --mode MODE           left|right|sum|stereo (default stereo)
  --bitrate BPS         Opus bitrate (default 64000)
  --music               tune Opus for music instead of voice
  --freq HZ             tone frequency of the left channel (default 440)
  --stall-at S          stop delivering audio at S seconds ...
  --stall-for S         ... for S seconds (default 0)
  --metrics S           metrics interval in seconds (default 10)
  --log-file            also write to the shared log file
";

struct Args {
    settings: StreamSettings,
    seconds: f64,
    rate: u32,
    block: usize,
    drift_ppm: f64,
    mode: ChannelMode,
    freq: f32,
    stall_at: Option<f64>,
    stall_for: f64,
    metrics: f64,
    log_file: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        settings: StreamSettings {
            token: std::env::var("WHIP_TOKEN").unwrap_or_default(),
            ..Default::default()
        },
        seconds: 30.0,
        rate: 48_000,
        block: 512,
        drift_ppm: 0.0,
        mode: ChannelMode::Stereo,
        freq: 440.0,
        stall_at: None,
        stall_for: 0.0,
        metrics: 10.0,
        log_file: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        fn num<T: std::str::FromStr>(flag: &str, v: String) -> Result<T, String> {
            v.parse()
                .map_err(|_| format!("invalid value for {flag}: {v}"))
        }
        match flag.as_str() {
            "--endpoint" => a.settings.endpoint = value()?,
            "--token" => a.settings.token = value()?,
            "--seconds" => a.seconds = num(&flag, value()?)?,
            "--rate" => a.rate = num(&flag, value()?)?,
            "--block" => a.block = num(&flag, value()?)?,
            "--drift-ppm" => a.drift_ppm = num(&flag, value()?)?,
            "--bitrate" => a.settings.bitrate = num(&flag, value()?)?,
            "--music" => a.settings.content = Content::Music,
            "--freq" => a.freq = num(&flag, value()?)?,
            "--stall-at" => a.stall_at = Some(num(&flag, value()?)?),
            "--stall-for" => a.stall_for = num(&flag, value()?)?,
            "--metrics" => a.metrics = num(&flag, value()?)?,
            "--log-file" => a.log_file = true,
            "--mode" => {
                a.mode = match value()?.as_str() {
                    "left" => ChannelMode::MonoLeft,
                    "right" => ChannelMode::MonoRight,
                    "sum" => ChannelMode::MonoSum,
                    "stereo" => ChannelMode::Stereo,
                    m => return Err(format!("unknown mode {m}")),
                }
            }
            "-h" | "--help" => return Err(String::new()),
            f => return Err(format!("unknown option {f}")),
        }
    }
    if !a.settings.is_complete() {
        return Err("--endpoint must be an http(s) URL".into());
    }
    if a.block == 0 || a.rate == 0 {
        return Err("--rate and --block must be positive".into());
    }
    Ok(a)
}

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.target() == "whip_send" && m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("{:5} {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let _ = log::set_logger(&StderrLogger);
    log::set_max_level(log::LevelFilter::Info);

    let shared = Shared::new();
    shared.set_sample_rate(args.rate);
    shared.set_channel_mode(args.mode);
    let settings = Arc::new(RwLock::new(args.settings.clone()));
    let (mut feeder, consumer) = audio_channel(shared.clone());
    let sender = match Sender::spawn(
        settings,
        shared.clone(),
        consumer,
        SenderOptions {
            metrics_interval: Duration::from_secs_f64(args.metrics.max(1.0)),
            log_to_file: args.log_file,
        },
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot start sender: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The simulated host clock runs at rate * (1 + ppm), so each block takes
    // correspondingly less (or more) real time.
    let block_period = Duration::from_secs_f64(
        args.block as f64 / (args.rate as f64 * (1.0 + args.drift_ppm * 1e-6)),
    );
    let mut left = vec![0.0f32; args.block];
    let mut right = vec![0.0f32; args.block];
    let (mut phase_l, mut phase_r) = (0.0f32, 0.0f32);
    let step_l = TAU * args.freq / args.rate as f32;
    let step_r = step_l * 1.5;
    let start = Instant::now();
    let mut next = start;
    let mut stalled = false;

    loop {
        let t = start.elapsed().as_secs_f64();
        if args.seconds > 0.0 && t >= args.seconds {
            break;
        }
        let in_stall = args
            .stall_at
            .is_some_and(|s| t >= s && t < s + args.stall_for);
        if in_stall {
            if !stalled {
                eprintln!(
                    "INFO  [cli] simulating a host stall for {} s",
                    args.stall_for
                );
                stalled = true;
            }
            feeder.reset_timing();
            thread::sleep(Duration::from_millis(10));
            next = Instant::now();
            continue;
        }
        stalled = false;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            *l = 0.3 * phase_l.sin();
            *r = 0.3 * phase_r.sin();
            phase_l = (phase_l + step_l) % TAU;
            phase_r = (phase_r + step_r) % TAU;
        }
        feeder.push(&left, &right);
        next += block_period;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else if now - next > Duration::from_secs(1) {
            // Catch up after short scheduling hiccups like a steady sound
            // card clock would; only give up after a long stall.
            next = now;
        }
    }

    let status = shared.status();
    drop(sender);
    eprintln!(
        "done: state={} encoded={} packets={} silence={} trimmed={} overruns={} drift_ppm={}",
        status.state.as_str(),
        status.frames_encoded,
        status.packets_sent,
        status.silence_frames,
        status.trimmed_frames,
        status.overrun_frames,
        status
            .drift_ppm
            .map(|d| format!("{d:.1}"))
            .unwrap_or_else(|| "-".into())
    );
    if status.packets_sent > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
