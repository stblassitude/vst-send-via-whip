# Architecture overview

## Goals

- **Must have**
    - Runs in Reaper on Windows x64.
    - Each instance has its own endpoint and token.
    - Mono or stereo, Opus at 48 kHz.
    - Reconnects automatically.
    - Never blocks or glitches the audio thread.
- **Should have**
    - Also builds for macOS and Linux.
    - Settings, including the token, are stored in the project file.
    - Connection status and a level meter in the editor.
    - A send on/off control that can be automated.
- **Out of scope**
    - Receiving audio (WHEP).
    - Video.
    - Multiple endpoints per instance.

Robustness during multi-hour live events is the overriding concern. The
design rules in [Audio path and clocking](audio-path.md) come from a failure
at a previous event.

## Components

```
┌─────────────────────── host process (Reaper) ────────────────────────┐
│                                                                      │
│  audio thread               worker thread ("whip-send", per instance)│
│  ────────────               ───────────────────────────────────────  │
│  process()                  every ≤5 ms:                             │
│   • pass audio through        • drain ring buffer, trim if > 150 ms  │
│   • Feeder::push() ──SPSC──►  • channel mode, gain, peak             │
│     (copy, count, no wait)    • resample to 48 kHz (if needed)       │
│                               • Opus encode 20 ms frames             │
│  editor (GUI thread)          • str0m: RTP/RTCP, ICE, DTLS/SRTP      │
│   • settings → RwLock ─────►  • UDP sockets (non-blocking)           │
│   • status   ◄── Mutex ─────  • WHIP state machine, reconnects       │
│                               • status every 200 ms, metrics 60 s    │
│                                                                      │
│                             short-lived threads: WHIP POST / DELETE  │
└──────────────────────────────────────────────────────────────────────┘
```

| Crate | Role |
|---|---|
| `whip-core` | Everything that is not plugin-specific. `Feeder` (audio-thread side), `Sender` (owns the worker thread), `Shared` (atomics and status), `StreamSettings`. |
| `whip-send-plugin` | nih-plug wrapper. Parameters, persisted settings, the egui editor, and a standalone binary. |
| `whip-send-cli` | Test sender that drives `whip-core` like a host would. |

## Threads and lifetimes

- **Audio thread.** It only calls `Feeder::push()`, which:
    - copies samples into a pre-allocated lock-free ring buffer (`rtrb`,
      2 s at 192 kHz stereo);
    - updates atomic counters, including callback timing.

    It never allocates, locks, logs or does I/O. If the buffer is full, frames
    are dropped and counted.
- **Worker thread.** One per plugin instance. It is started in
  `initialize()`, not when the plugin is constructed, so hosts that only scan
  plugins never start network activity. It is woken by str0m's timeouts, or
  at least every 5 ms.
- **WHIP requests** run on short-lived threads with timeouts (POST 5 s,
  DELETE 2 s), so the worker keeps draining audio while HTTP is in flight.
  The DNS lookup that decides whether loopback addresses are used also runs
  on a helper thread; the worker waits at most 1 s for it.
- **Panics.** The worker loop runs under `catch_unwind`. A panic is logged,
  the session and the audio pipeline are discarded, and the worker
  reconnects as after any other failure.
- **Shutdown.** Dropping the `Sender`:
    1. signals the worker to stop;
    2. the worker ends the WHIP session (DELETE on a helper thread);
    3. it waits up to 500 ms for helper threads, then leaves the rest
       to finish or time out on their own;
    4. the `Sender` then joins the worker.

    The host's main thread waits at most about 1.5 s, even when the server
    is unreachable or DNS hangs. Helper threads may outlive the plugin
    instance, so the plugin binary is pinned in memory (`RTLD_NODELETE` or
    `GET_MODULE_HANDLE_EX_FLAG_PIN`) when the first sender starts: the host
    can't unload code that is still running.

## Shared state

| What | From → to | Mechanism |
|---|---|---|
| Audio samples | audio thread → worker | SPSC ring buffer (`rtrb`) |
| Enabled, gain, channel mode, sample rate | parameters → worker | atomics, set from parameter callbacks and `initialize()`/`process()` |
| Callback count, frames, overruns, interval statistics | audio thread → worker | atomics |
| Endpoint, token, bitrate, content | editor or loaded state → worker | `Arc<RwLock<StreamSettings>>`, persisted by nih-plug; the worker polls it with `try_read()` |
| Status and metrics | worker → editor | `Mutex<Status>` snapshot, published every 200 ms |

Each instance has its own state, with no process-wide singletons other than
the shared log file. This works both when Reaper loads all instances into
one process and when it runs plugins in separate processes.

## Further reading

- [Technology choices](technology.md): why Rust, nih-plug, str0m and libopus.
- [Audio path and clocking](audio-path.md): real-time rules, timing,
  drift, and the lessons from listen-ingest.
- [WHIP and WebRTC](whip-webrtc.md): signalling, session handling, Opus
  settings, reconnects.
- [Original proposal](proposal.md): the design document written before
  implementation, kept for reference.
