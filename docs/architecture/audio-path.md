# Audio path and clocking

## Lessons from listen-ingest

At a previous event, the standalone ingest tool
[listen-ingest](https://codeberg.org/c3lingo/listen-ingest) degraded over
30–45 minutes until no audio came through. The capture device was a **Dante
Virtual Soundcard (DVS)**; with a plain USB audio interface, the problem could
not be reproduced.

### Analysis

**Clock drift between sender and listeners was not the cause.**
listen-ingest timestamps its RTP packets by counting samples. Listeners play
the stream through the browser's WebRTC playout buffer (NetEQ), which
compensates for drift by time-stretching. A typical 100 ppm mismatch adds
up to about 0.27 s over 45 minutes, well within what NetEQ handles. And
the network, Galène and the listeners were the same in the working USB
setup.

**The likely cause is on the capture side, where DVS adds a second clock.**

- A USB interface has one clock, its crystal. The driver delivers samples at
  exactly that rate.
- DVS has no hardware. Its sample clock follows the Dante network's PTP
  leader, but the software runs on the host's scheduler and timers, which
  use the computer's crystal.
- DVS bridges the two with an internal buffer. If that bridge drifts, the
  buffer slowly creeps toward empty or full. Delivery becomes uneven, then
  drops out, and finally stalls.
- Unstable PTP on the host (Wi-Fi, USB Ethernet, Energy-Efficient Ethernet),
  the shared WDM/WASAPI path instead of ASIO, and small DVS latency settings
  make this worse.

This is a hypothesis that fits the evidence, not a measurement. That is why
WHIP Send measures exactly the quantities that would confirm it.

**listen-ingest amplified the problem:**

1. Its capture callback handed a driver-owned buffer to another goroutine
   without copying it, and blocked when the consumer was slow.
2. Without WebRTC interceptors there were no RTCP sender reports, no
   retransmissions, and no evaluation of receiver reports.
3. Without metrics, the failure could not be diagnosed after the fact.

### Design rules

These rules follow from the analysis. The rest of this page describes how
they are implemented.

1. **One clock domain.** The host's audio device is the only clock; RTP
   timestamps follow the sample count.
2. **The audio thread never blocks and never shares buffers.**
3. **Bounded latency** with a defined way to recover.
4. **Complete RTCP handling.**
5. **Built-in metrics,** logged and shown, from the first prototype.
6. **Long soak tests** with the real setup before every event.

## Real-time rules

`process()` calls `Feeder::push(left, right)`, and that is all it does:

- It records the time since the previous callback, as atomic min, max, sum
  and sum of squares.
- It adds the block's frames to an atomic counter.
- It copies the samples as interleaved stereo into the ring buffer. Mono
  tracks are pushed as L = R.
- If the buffer has no room, the excess frames are dropped and counted as
  overruns.

There is no allocation, no lock, no logging and no system call (other than
reading the monotonic clock). The plugin reports zero latency, and the
track's audio is passed through untouched. Offline renders are not pushed
at all.

## Data-driven timing

The worker never paces audio by the system clock while audio is flowing:

1. Each tick it drains everything in the ring buffer.
2. It applies the channel mode and gain. Gain is ramped linearly across the
   chunk, so automation doesn't click.
3. It resamples to 48 kHz if needed, and appends to the Opus framer.
4. Every complete 960-sample frame (20 ms) is encoded and sent at once. Its
   RTP timestamp advances by exactly 960.

So the stream's timing *is* the host's audio clock. Bursty delivery from the
host becomes bursty sending, which the receivers' jitter buffers absorb.
Galène and the browsers follow the host clock, whatever its offset.

## When the host stops delivering

If no audio arrives for max(200 ms, 3 host blocks), the worker considers the
input stalled. This happens when the transport is stopped, the track is not
processed, or the audio engine is closed. The worker then:

- logs it once;
- drops any partially filled frame;
- sends 20 ms frames of digital silence **paced by the system clock**, so the
  WebRTC session stays alive and listeners hear silence rather than a frozen
  stream.

After a long scheduling gap it doesn't catch up on more than 200 ms of
silence. When audio returns, real frames continue with the next timestamp.

## Bounded latency: trimming

Each tick, the worker checks how much audio is waiting in the ring buffer.
If more than max(150 ms, 2 host blocks) is queued, it discards the oldest
audio down to max(40 ms, 1 host block). The discarded frames are counted as
`trimmed_frames`, and a warning is logged (rate-limited, and only while
live).

Normally the queue never holds more than about one host block, because the
worker drains it every few milliseconds. The queue can only grow if:

- the worker was starved of CPU; or
- the host delivered a burst, for example when resuming after a stall.

In both cases the right answer is to continue from the newest audio, not to
play out a backlog and carry the extra latency forever.

## Measuring clock drift

`drift_ppm` compares the host's effective sample rate with the computer's
monotonic clock. A simple "frames ÷ elapsed time" is off by up to one host
block. That is ±900 ppm after 25 s at 1024 frames and 44.1 kHz: useless for
diagnosing a DVS that is off by 50 ppm.

Instead, the worker records the pair (time, cumulative frames delivered) on
every tick that had input. It fits a straight line through these points with
an online least-squares fit (Welford's method, numerically stable over hours):

- the slope is the effective rate in frames per second;
- block quantization and tick jitter average out.

The estimate is reported after 10 s of continuous input. It restarts after a
stall or a sample-rate change.

In tests with three simultaneous simulated hosts (0, +150 and −80 ppm), the
estimate was within 5 ppm after 30 s, and within 1 ppm after 10 minutes.

## Metrics

The worker publishes a status snapshot to the editor every 200 ms and writes a
metrics line to the log every 60 s. The fields are described in
[Monitoring and troubleshooting](../usage/troubleshooting.md#reading-the-metrics-line).
They are chosen so that a degradation can be attributed:

- `cb_ms` and `drift_ppm` → the host's audio clock and callbacks;
- `queue_max_ms`, `overrun_frames` and `trimmed_frames` → the hand-over
  between the audio thread and the worker;
- `loss`, `jitter_ms` and `rtt_ms` (from Galène's RTCP receiver reports) →
  the network.

## Resampling and channel modes

- The channel mode (mono left, right, L+R average, or stereo) is applied
  **before** resampling, so mono streams only resample one channel.
- If the host runs at 48 kHz, audio passes through unchanged.
- Otherwise, a rubato sinc resampler (128 taps, squared Blackman window)
  converts in fixed 256-frame chunks.
- A change of sample rate or channel mode rebuilds the pipeline. The WebRTC
  session stays up, and the RTP timestamps continue.
