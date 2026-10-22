# Proposal: "WHIP Send" audio plugin for Reaper

!!! info "Historical document"
    This is the design proposal written before implementation, kept for
    reference. Some details changed during implementation; the other
    [architecture pages](index.md) describe the plugin as built.

A plugin that sits on a Reaper track (or bus), passes the audio through
unchanged, and publishes a copy of it to a WHIP endpoint such as a Galene group.
For our use case, three instances on three tracks publish to three Galene
groups.

This expands Option 1 of the earlier solution suggestions (see [Technology choices](technology.md#approaches-considered)).

## Requirements

- **Must have**
  - Runs in Reaper on Windows x64.
  - Each instance has its own endpoint and token.
  - Mono or stereo, Opus at 48 kHz.
  - Reconnects automatically.
  - Never blocks or glitches the audio thread.
- **Should have**
  - Builds for macOS and Linux too.
  - Tokens can be stored in the `.rpp` project file.
  - Shows connection status and a level meter in the UI.
  - A send on/off control that Reaper can automate or trigger from actions/OSC.
- **Out of scope**
  - Receiving audio (WHEP).
  - Video.
  - Multiple endpoints per instance.

## Plugin format

| Format | Reaper support | Notes |
|---|---|---|
| **VST3** | Full | The universal choice. Since VST 3.8 (Oct 2025) the SDK is MIT-licensed, so licensing is no longer an obstacle. |
| **CLAP** | Full (since 6.71) | Modern, simple C ABI, MIT-licensed, good threading model. |
| VST2 | Full | The SDK is no longer distributed. Don't use it. |
| AU / LV2 | macOS only / Linux only | Only needed if other DAWs matter. |

**Recommendation: ship VST3 and CLAP from the same code base.** Both
frameworks below can do this.

## Option A (recommended): Rust

| Concern | Library | Why |
|---|---|---|
| Plugin framework | [nih-plug](https://github.com/robbert-vdh/nih-plug) | Builds VST3 and CLAP from one crate for Windows, macOS and Linux, packaged with `cargo xtask bundle`. Supports parameters and persisted non-parameter state. |
| GUI | `nih_plug_egui` (egui) | Easy text fields for endpoint and token (masked), plus status text and a meter. Alternatives are vizia and iced. |
| WebRTC | [str0m](https://github.com/algesten/str0m) | Sans-IO and synchronous: you own the UDP socket and the loop, so no async runtime runs inside the DAW process. Has Opus RTP packetization, RTCP and ICE-lite/full. |
| Alternative WebRTC | [webrtc-rs](https://github.com/webrtc-rs/webrtc) | A more complete port of Pion, but it needs a Tokio runtime per process. It is heavier and harder to tear down cleanly when a plugin is unloaded. |
| Opus | [`audiopus`](https://crates.io/crates/audiopus) / `opus` crate (bindings to libopus) | libopus is the reference encoder, so there is no reason to use anything else. `audiopus_sys` builds and statically links libopus through CMake. |
| Resampling | [`rubato`](https://crates.io/crates/rubato) | Needed when the project runs at 44.1 or 96 kHz. Pure Rust, real-time quality. |
| HTTP (WHIP) | [`ureq`](https://crates.io/crates/ureq) with rustls | Blocking, small, no OpenSSL. WHIP needs only POST, DELETE and optionally PATCH. |
| Audio → worker queue | [`rtrb`](https://crates.io/crates/rtrb) | Lock-free, wait-free SPSC ring buffer, safe to use on the audio thread. |

**Pros**
- Memory safety in a process we must not crash (Reaper during a live show).
- One `cargo build` produces a self-contained DLL with libopus and rustls
  statically linked, so there are no extra DLLs to install next to the plugin.
- The same `whip-core` crate can power the standalone ReaStream helper
  (Option 2 in the suggestions document). The work is never wasted, whichever
  route ends up in production.

**Cons / risks**
- nih-plug's VST3 bindings (`vst3-sys`) are GPLv3, so VST3 builds must be
  GPL-compatible. That is fine for an open-source project. CLAP builds are
  unaffected. This may change now that the VST3 SDK itself is MIT; check the
  current nih-plug README.
- nih-plug development moves slowly, though it is stable in practice.
- str0m has less production WHIP mileage than libdatachannel (see Option B).
  Check its DTLS crypto backend feature flags so you can avoid a dynamic
  OpenSSL dependency on Windows.

## Option B: C++

| Concern | Library | Why |
|---|---|---|
| Plugin framework | **JUCE 8** (AGPLv3 or commercial) | The most productive C++ framework: VST3, AU, LV2, standalone, and a GUI toolkit included. CLAP via `clap-juce-extensions`. |
| Alternative framework | [CLAP SDK](https://github.com/free-audio/clap) + [clap-wrapper](https://github.com/free-audio/clap-wrapper), or [iPlug2](https://github.com/iPlug2/iPlug2) | Permissive licenses, but more manual GUI work. |
| WebRTC | [libdatachannel](https://github.com/paullouisageneau/libdatachannel) (MPL-2.0) | **OBS Studio's WHIP output is built on it**, so it is proven against many WHIP servers. Small, with no Google libwebrtc. Includes an Opus RTP packetizer and RTCP SR/RR handlers. |
| Opus | libopus | Same reasons as Option A. |
| Resampling | speexdsp resampler or libsamplerate | Both small and real-time capable. |
| HTTP (WHIP) | libcurl (as OBS uses), or `juce::URL` | |
| Build | CMake + vcpkg, static triplet (`x64-windows-static`) | libdatachannel needs OpenSSL, GnuTLS or Mbed TLS. Link it statically, because DLLs next to a plugin are not on the DLL search path. |

**Pros:** follows OBS's proven WHIP stack. JUCE is the industry standard for
plugins.

**Cons:** the usual C++ memory-safety risk inside the DAW process. The
dependency and static-linking setup on Windows takes more effort. The code is
not directly reusable for a ReaStream helper unless you also write it in C++.

**Not recommended:** Google's libwebrtc. It is huge, hard to build, brings its
own audio device module, and is far too much for sending one Opus track.

## Architecture (applies to both options)

```
 Audio thread (process())            Worker thread (one per instance)
 ───────────────────────            ─────────────────────────────────────
 pass audio through unchanged        loop:
 optional downmix / channel pick       pop samples from ring buffer
 push f32 samples ──► SPSC ring ──►    resample to 48 kHz (if needed)
 update peak meter (atomic)            accumulate 20 ms = 960 samples/ch
 read "enabled" param (atomic)         opus_encode → RTP (str0m / libdatachannel)
                                       drive ICE/DTLS/SRTP, RTCP
                                       WHIP state machine & reconnects
                                       publish status (atomic / lock-free)
```

### Real-time rules

- `process()` may only copy samples into the ring buffer, update atomics and
  pass audio through. It must not allocate, lock, log or touch the network.
- If the ring buffer is full because the worker is stalled, drop samples and
  count an overrun. Never wait.
- The plugin reports **zero latency**: the send is a side branch, and the
  track's own audio is untouched.

### Timing and clock

- Drive the encoder **by data, not by a wall-clock timer**. Encode whenever
  960 samples are available, and derive RTP timestamps from the sample count.
  This follows Reaper's audio clock, so drift is not an issue.
- If no audio arrives for about 100 ms (transport stopped, track not
  processed, Reaper's audio engine closed), the worker inserts silence frames,
  paced by wall clock, so the WebRTC session stays alive. Opus DTX can make
  these frames almost free.
- Tell operators to disable **anticipative FX processing** on these tracks.
  Otherwise Reaper may render the audio ahead of time in bursts.
- If the host sample rate or block size changes (`initialize()` /
  `setupProcessing()`), re-create the resampler. The WebRTC session can stay
  up.

### WHIP client (RFC 9725)

1. Create a peer connection with one **sendonly** audio transceiver (Opus
   48000/2, plus `useinbandfec=1`, and `stereo=1` if sending stereo).
2. Gather ICE candidates fully (non-trickle), then `POST <endpoint>` with
   `Content-Type: application/sdp` and `Authorization: Bearer <token>`.
3. Expect `201 Created`. Apply the SDP answer and store the `Location`
   header, which is the session URL. Follow 307/308 redirects, and apply any
   `Link: <…>; rel="ice-server"` headers as STUN/TURN servers.
4. While running, watch ICE/DTLS state and RTCP receiver reports (loss, RTT)
   and show them as status.
5. On failure, `DELETE` the old session as a best effort, then reconnect with
   exponential backoff capped at about 5 s. Optionally use ICE restart via
   `PATCH`, but a fresh POST is simpler and Galene handles it fine.
6. On plugin unload or "enabled = off", `DELETE` the session URL and stop the
   worker. Join the thread with a timeout so a hung network call can't freeze
   Reaper when it closes the project.

Galene specifics:
- The endpoint is `https://host[:port]/group/<name>/.whip`.
- Authentication is by Bearer token only, and the token needs `present`
  permission.
- The group must allow Opus. Audio-only offers are fine.

### Opus settings

| Setting | Speech / commentary | Music / programme |
|---|---|---|
| Application | `OPUS_APPLICATION_VOIP` | `OPUS_APPLICATION_AUDIO` |
| Bitrate | 32–64 kbit/s mono | 96–128 kbit/s stereo |
| Frame size | 20 ms | 20 ms (10 ms if latency is critical) |
| In-band FEC | on (packet-loss estimate of about 5–10 %) | on |
| DTX | optional | off |
| Complexity | 10 (trivial CPU cost for three streams) | 10 |

### Parameters, state and configuration

- **Automatable parameters**
  - `Send enabled` (bool): lets Reaper actions, automation or OSC switch a
    feed on and off live.
  - `Send gain` (dB).
  - `Channel mode`: mono L, mono R, mono sum, or stereo.
- **Persisted state, stored in the project**
  - Endpoint URL, token, bitrate and Opus application, kept as non-parameter
    plugin state (`#[persist]` fields in nih-plug, or the state chunk in
    JUCE/CLAP). The `.rpp` file is therefore self-contained: open the project
    and all three feeds are configured.
  - Reaper stores plugin state base64-encoded but **not encrypted**. Treat
    project files, backups and auto-saves as containing secrets, and don't
    commit them to public repositories.
  - Rotating a token means editing it in each plugin instance and saving the
    project.
  - Reaper's own FX presets (save, load and copy between tracks) store the
    same state, so a per-feed preset is a cheap way to set up a new project.
- **GUI**
  - Endpoint and token text fields. Mask the token, with a "show" toggle.
  - State indicator: disconnected, connecting, live, or error, with the HTTP
    status or ICE failure.
  - Live time, packets sent, loss and RTT.
  - Peak meter.
  - Reconnect button.

### Process and instance considerations

- Reaper loads all instances into one process unless plugin bridging or
  isolation ("run as separate process") is enabled. The design must work
  either way, so use **no global singletons that assume a single instance**.
  One worker thread per instance is simple and cheap for three streams.
- Each instance has its own UDP sockets. No fixed ports are needed, since ICE
  picks ephemeral ones.
- Log to a rotating file in `%LOCALAPPDATA%\WhipSend\logs`, never to stdout.
  Never log the token or the `Authorization` header.

## Lessons from listen-ingest: clocking and robustness

### What happened

At the last event, [listen-ingest](https://codeberg.org/c3lingo/listen-ingest)
audio degraded over 30–45 minutes until it stopped entirely. Clock skew was
suspected, but the cause was never confirmed.

- The capture device was a **Dante Virtual Soundcard (DVS)**.
- With a plain USB audio interface, the problem **could not be reproduced**.

### Analysis

**Sender-to-listener skew is not the cause.**

- Each Opus frame's RTP timestamp advances by a fixed 960 samples, and the
  encoder buffers to exactly 960 samples. Timestamps therefore follow the
  capture device's clock, not the system clock.
- listen-frontend plays the stream through a plain `<audio>` element. The
  browser's WebRTC playout buffer (NetEQ) compensates for sender/receiver
  drift by time-stretching. A typical 100 ppm mismatch adds up to about
  0.27 s over 45 minutes, well within what NetEQ absorbs.

The USB-interface result confirms this: the WebRTC path, Galene and the
listeners were the same in both setups, and only the capture device changed.

**The problem is on the capture side, where DVS brings in a second clock.**

- A USB interface has a single clock: its own crystal. The driver delivers
  samples at exactly the rate the device produces them, so the capture
  callback is a faithful, evenly paced clock.
- DVS has no hardware. Its sample clock is disciplined to the Dante network's
  PTP leader, but the work is done in software on the host. The host's audio
  stack, scheduler and timers run from the computer's own crystal.
- DVS bridges the two clock domains with an internal buffer. If that bridge
  drifts, its fill level slowly creeps toward empty or full. Delivery then
  becomes uneven, the consumer sees short or late chunks, then dropouts, and
  finally the stream stalls. This matches the 30–45 minute pattern.
- Factors that make this worse:
  - unstable PTP on the host: Wi-Fi, a USB-Ethernet adapter, Energy-Efficient
    Ethernet or NIC power saving, no QoS on the switch;
  - DVS used through the OS's shared audio path (WDM/WASAPI shared mode on
    Windows) rather than ASIO or Core Audio directly;
  - small DVS latency settings;
  - **a consumer that is itself fragile under jitter**, which listen-ingest
    was (see defects 1 and 3 below).

DVS's buffer drift is a hypothesis that fits the evidence, not a measurement.
It can be confirmed with the metrics from design rule 5:
- the effective capture sample rate measured against the system clock,
- callback-interval jitter and chunk sizes,
- DVS sync status and clock-offset history in Dante Controller.

Defects found in listen-ingest that amplify an uneven source like DVS:

1. **Shared buffer handed off without copying, from the audio callback.**
   - The malgo capture callback (in the forked pion/mediadevices,
     `pkg/driver/microphone/microphone.go`) puts miniaudio's `chunk` slice
     into a 1-slot channel without copying it.
   - miniaudio reuses that memory after the callback returns, so the encoder
     can read samples that have already been overwritten.
   - When the sending side stalls, the callback blocks, and the OS drops
     capture buffers.
2. **No WebRTC interceptors.** `webrtc.NewAPI` is called with only a media
   engine, so there are:
   - no RTCP sender reports,
   - no retransmission when Galene reports lost packets (NACK),
   - no evaluation of Galene's receiver reports.

   Packet loss is never repaired, and nothing records how the stream is
   doing.
3. **No metrics.** Frame counts, queue depths and loss are never logged, which
   is why the failure couldn't be diagnosed at the event.

### Design rules for the plugin

1. **One clock domain.** Reaper's audio device is the only clock.
   - Samples enter the plugin on that clock, and RTP timestamps are derived
     from the sample count.
   - While audio is flowing, nothing is paced by the system clock.
   - There is no second audio device, virtual cable or loopback anywhere in
     the chain.
   - **Choosing Reaper's audio device for Dante:**
     - *Preferred:* a hardware Dante interface (PCIe card, or a Dante-to-USB
       adapter or interface). Its sample clock is locked to PTP in hardware,
       and the host sees it as an ordinary single-clock device, like the USB
       interface that worked.
     - *If DVS is used:* make it Reaper's **only** device, via its **ASIO**
       driver, so Reaper's engine runs directly on DVS's clock and the plugin
       inherits it. Use a wired onboard NIC with EEE and power saving off, a
       moderate DVS latency (≥ 4–6 ms), and verify sync in Dante Controller.
       Never combine DVS with another interface or a virtual cable.
     - Run the soak test (rule 6) with the exact Dante setup used at the
       event. A USB interface on the bench does not exercise this failure
       mode.
2. **The audio thread never blocks or shares buffers.**
   - `process()` copies samples into a pre-allocated lock-free SPSC ring
     buffer (`rtrb`, see above).
   - If the buffer is full, samples are dropped and an overrun counter is
     incremented. The audio thread never waits.
   - No buffer owned by the host or driver is used after the callback
     returns.
3. **Bounded latency, with a defined way to recover.**
   - The ring buffer holds about 200–500 ms.
   - The worker checks queue depth. If it stays above a high-water mark (for
     example 100 ms) for more than a few seconds, the worker discards the
     oldest samples down to the target and increments a "trimmed" counter.
     Latency can never grow without limit.
   - This matters most at the hand-over between the two modes: silence
     generated on the system clock while Reaper isn't delivering, and real
     audio paced by Reaper's clock. When audio resumes, start again from the
     newest samples. Never play out a backlog.
4. **Complete RTCP handling.**
   - Send RTCP sender reports.
   - Answer NACKs from a short history of recently sent packets (about 1 s).
   - Read Galene's receiver reports (loss, jitter, RTT) and surface them.
   - str0m and libdatachannel provide all of this. If Pion is ever used, call
     `webrtc.RegisterDefaultInterceptors`.
5. **Built-in metrics, from the first prototype.** Log per instance every 60 s
   and show the live values in the GUI:
   - samples received vs. elapsed system time, i.e. the effective sample rate
     and drift in ppm,
   - block size and the interval between `process()` calls (min, max,
     standard deviation). Uneven delivery from DVS shows up here first,
   - ring-buffer depth (min, avg, max over the interval),
   - overruns, silence frames inserted, samples trimmed,
   - Opus frames encoded, RTP packets and bytes sent, NACKs answered,
   - Galene's reported fraction lost, jitter and RTT.

   With these numbers, a future degradation can be attributed directly:
   - queue depth trending means a clock or throughput problem,
   - overruns mean the worker is starved,
   - rising loss means a network problem.
6. **Long soak tests as a release gate.**
   - Before each event, run all three instances for at least 2–3 hours
     against the production Galene, with a listener in Chrome.
   - Check `chrome://webrtc-internals` on the listener: `concealedSamples`
     should stay near zero, and `jitterBufferDelay` should stay flat.
     `insertedSamplesForDeceleration` and `removedSamplesForAcceleration`
     show how much drift correction is happening.
   - Check the plugin's own metrics log for trends.

## Platform matrix

| Platform | Status | Notes |
|---|---|---|
| Windows x64 | Primary target | MSVC toolchain. Static CRT is optional. No code signing needed for plugins. Install to `C:\Program Files\Common Files\VST3` and `…\CLAP`. |
| Windows ARM64 | Possible | Reaper has ARM64EC builds. Both stacks cross-compile. Low priority. |
| macOS (arm64 + x86_64) | Secondary | Universal binary. Must be signed and notarized, or users must clear the quarantine flag. |
| Linux x64 | Secondary | Mainly useful for CI and development testing against a local Galene. |

## Repository layout (Rust option)

```
whip-send/
  crates/
    whip-core/        # WHIP client + Opus encoder + resampler, no plugin deps
    whip-send-cli/    # test tool: WAV file or sine → WHIP; later the ReaStream helper
    whip-send-plugin/ # nih-plug wrapper: ring buffer, params, egui UI
  xtask/              # nih-plug bundler
  .github/workflows/  # build VST3 + CLAP for windows/macos/linux, run whip-core tests
```

## Development plan

1. **`whip-core` + CLI (about 2–3 days).** Publish a sine wave or WAV file to
   a local Galene and check it in the Galene web client: connect, disconnect,
   token errors, reconnect after restarting Galene, and packet loss using
   `clumsy` on Windows or `tc netem` on Linux.
2. **Plugin wrapper (about 2 days).** Ring buffer, parameters, persisted
   endpoint and token state, minimal egui settings and status UI. Test in
   Reaper with three instances, including save, reload and copying presets
   between tracks.
3. **Hardening (about 2–3 days).**
   - Sample-rate changes, transport stop/start, project close while
     connected, offline render (don't send, or optionally send), and
     overnight soak tests checked against the metrics described in
     "Lessons from listen-ingest".
   - CPU and latency measurements.
   - Run under Reaper with the audio buffer at 64 samples to catch real-time
     violations.
4. **Packaging.** CI artifacts and an install and setup guide.

Step 1 also covers most of the ReaStream helper (Option 2). Only the UDP
ReaStream receiver is missing there, so the plugin and helper paths share
almost all of the risk-reduction work.

## Recommendation

Use **Rust + nih-plug (VST3 + CLAP) + str0m + libopus (`audiopus`) + rubato +
ureq/rustls**, structured around a reusable `whip-core` crate.

If str0m turns out to cause problems against Galene or behind tricky NATs,
switch the WebRTC layer to **libdatachannel**, which is OBS's proven WHIP
stack. Either do this through a thin FFI from Rust, or move to Option B (C++,
JUCE + libdatachannel + libopus).
