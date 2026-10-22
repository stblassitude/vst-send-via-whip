# WHIP Send

[![CI](https://github.com/stblassitude/vst-send-via-whip/actions/workflows/ci.yml/badge.svg)](https://github.com/stblassitude/vst-send-via-whip/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/stblassitude/vst-send-via-whip)](https://github.com/stblassitude/vst-send-via-whip/releases/latest)

An audio plugin (VST3 and CLAP) that publishes a track's audio to a WHIP
endpoint such as a [Galène](https://galene.org/) group, while passing the
audio through unchanged. One plugin instance per track or bus sends one
stream, so three instances on three tracks feed three Galène groups.

Background and design are in [plugin-proposal.md](plugin-proposal.md). The
implementation follows its section *Lessons from listen-ingest: clocking and
robustness* in particular.

## Download

Prebuilt plugins for Windows (x64), macOS (universal) and Linux (x64) are
attached to each [release](https://github.com/stblassitude/vst-send-via-whip/releases).
Each archive contains `WHIP Send.vst3`, `WHIP Send.clap` and the
`whip-send-cli` test sender. To install, see the table under *Building*.
The macOS bundles are not notarized; after copying them, clear the
quarantine flag:

```sh
xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/"WHIP Send.vst3"
```

## Layout

| Path | Contents |
|---|---|
| `crates/whip-core` | Everything except the plugin wrapper: lock-free hand-over from the audio thread, channel mode/gain, resampling to 48 kHz, Opus encoding, WHIP client, WebRTC session (str0m), metrics and logging. |
| `crates/whip-send-plugin` | nih-plug wrapper (VST3 + CLAP), egui editor, and a standalone test binary. |
| `crates/whip-send-cli` | Test sender that simulates a host feeding a sine tone, with configurable sample rate, block size, clock drift and stalls. |
| `xtask` | nih-plug's bundler (`cargo xtask bundle`). |
| `scripts/local-galene.sh` | Builds and runs a local Galène for testing. |

## Building

```sh
git clone https://github.com/stblassitude/vst-send-via-whip.git
cd vst-send-via-whip
```

Requirements: a recent stable Rust (developed with 1.99) and CMake (used to build the
bundled libopus, which is linked statically). On macOS:
`brew install rust cmake`.

```sh
cargo xtask bundle whip-send-plugin --release
```

This produces `target/bundled/WHIP Send.vst3` and `target/bundled/WHIP Send.clap`.
The bundles depend on system libraries only.

Install by copying the bundle:

| OS | VST3 | CLAP |
|---|---|---|
| macOS | `~/Library/Audio/Plug-Ins/VST3/` | `~/Library/Audio/Plug-Ins/CLAP/` |
| Windows | `C:\Program Files\Common Files\VST3\` | `C:\Program Files\Common Files\CLAP\` |
| Linux | `~/.vst3/` | `~/.clap/` |

Windows and Linux builds pass CI but have not been tested in a host yet.

On Linux, building the editor needs the X11/OpenGL development packages:

```sh
sudo apt-get install cmake libasound2-dev libgl-dev libjack-dev libx11-xcb-dev \
  libxcb1-dev libxcb-dri2-0-dev libxcb-icccm4-dev libxcursor-dev libxkbcommon-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev
```

## Continuous integration and releases

- **[`ci.yml`](.github/workflows/ci.yml)** runs on every push and pull
  request. It checks formatting, runs clippy and the tests, and builds the
  plugin bundles on Linux, macOS and Windows.
- **[`release.yml`](.github/workflows/release.yml)** runs when a `v*` tag is
  pushed:
  1. It sets the version from the tag (`v1.2.3` becomes `1.2.3`). This is the
     version the VST3 and CLAP plugins report, and it is also written into the
     macOS bundles' `Info.plist`.
  2. It builds the bundles for all three platforms (universal for macOS).
  3. It creates a GitHub release with the archives attached and a list of the
     commits since the previous tag.

To release:

```sh
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

The version in `Cargo.toml` only matters for local builds. Releases always
take it from the tag.

## Using the plugin

Insert *WHIP Send* on the track (mono or stereo), open its editor, and:

1. Enter the endpoint, e.g. `https://galene.example.org:8443/group/voc-a/.whip`,
   and the token. The token needs the `present` permission; create one with
   `galenectl create-token -group voc-a -permissions present`, or with
   */invite* in the Galène UI.
2. Choose bitrate and content type (Voice or Music), then click **Apply**.
   Edits are only applied on **Apply**, so typing doesn't reconnect.
3. Choose the channel mode: mono from left, right or L+R, or stereo.

Endpoint, token and encoder settings are saved in the project file (and in
FX presets). Reaper stores plugin state encoded but **not encrypted**, so
treat project files, backups and auto-saves as containing secrets.

Automatable parameters: **Send enabled**, **Send gain** (−30…+12 dB) and
**Channels**. Switching *Send enabled* off ends the WHIP session; switching
it on starts a new one.

For live use, follow the rules in the proposal: one audio device as Reaper's
only clock (a hardware Dante interface, or DVS via ASIO only), no virtual
cables in the chain, and anticipative FX processing disabled on these tracks.
Offline renders are never streamed.

### Status and logs

The editor shows the connection state, a post-gain peak meter and the
metrics from the last minute.

Every instance writes events and a metrics line once a minute to a shared
log file. The token is never logged.

- macOS: `~/Library/Logs/WhipSend/whip-send.log`
- Windows: `%LOCALAPPDATA%\WhipSend\logs\whip-send.log`
- Linux: `~/.local/state/whip-send/whip-send.log`

A metrics line looks like this (wrapped here; this one is from a test with a
simulated host clock running 80 ppm slow):

```
metrics state=live rate=48000 ch=1 block=128 cb_ms(n=22499 min=0.00 avg=2.67 max=8.71 sd=0.51)
  drift_ppm=-79.8 queue_max_ms=16.0 overrun_frames=0 silence_frames=0 trimmed_frames=0
  encoded=14994 pkts=14960 bytes=2123892 nacks=0 rtt_ms=7.6 loss=0.000 jitter_ms=2.2 lost=0 reconnects=0
```

How to read it:

- **`cb_ms`**: the interval between the host's audio callbacks (count, min,
  average, max, standard deviation). Uneven delivery, for example from Dante
  Virtual Soundcard, shows up here first.
- **`drift_ppm`**: the host's effective sample rate against the system clock,
  from a least-squares fit since input last (re)started. It is shown after
  10 s of continuous input.
- **`queue_max_ms`**: the peak depth of the hand-over queue. Above about
  150 ms the oldest audio is discarded (`trimmed_frames`), so latency can't
  creep up.
- **`overrun_frames`**: audio the host delivered while the queue was full.
- **`silence_frames`**: 20 ms frames of silence sent while the host delivered
  nothing (transport stopped, track not processed), so the session stays
  alive.
- **`rtt_ms`, `loss`, `jitter_ms`, `lost`**: taken from Galène's RTCP receiver
  reports.

### Reconnecting

Failed or lost connections are retried with backoff (1, 2, 4, 5, 5… s).

A session counts as lost when ICE fails, or when nothing (STUN consent
checks, RTCP) has arrived from the server for 5 s. With a Galène restart this
means about 5 s plus the retry delay.

**Reconnect** in the editor forces a new session.

## Testing on macOS

Unit tests:

```sh
cargo test -p whip-core
```

They cover URL resolution, Opus framing, resampling, drift estimation,
interface selection and settings.

### Local Galène

```sh
scripts/local-galene.sh ~/src/galene          # serves http://127.0.0.1:8444/
```

### Test sender

The CLI simulates a host. For example, three streams with different host
conditions:

```sh
E=http://127.0.0.1:8444/group
cargo run --release -p whip-send-cli -- --endpoint $E/voc-a/.whip --seconds 60
cargo run --release -p whip-send-cli -- --endpoint $E/voc-b/.whip --rate 44100 --block 1024 --drift-ppm 150 --mode sum
cargo run --release -p whip-send-cli -- --endpoint $E/voc-c/.whip --block 128 --drift-ppm -80 --stall-at 20 --stall-for 3
```

Pass the token with `--token` or the `WHIP_TOKEN` environment variable. Listen
in a browser at `http://127.0.0.1:8444/group/voc-a/`, and check
`chrome://webrtc-internals` for `concealedSamples` and `jitterBufferDelay`.

### Standalone plugin

This runs the real plugin wrapper without a DAW:

```sh
cargo build --release -p whip-send-plugin --bin whip-send-standalone
WHIP_ENDPOINT=http://127.0.0.1:8444/group/voc-a/.whip \
  ./target/release/whip-send-standalone -b core-audio --input-device "<device name>"
```

- `WHIP_HEADLESS=1` runs it without the editor window.
- `-b dummy` feeds silence without touching audio devices.
- Using a CoreAudio input requires microphone permission for the terminal.

### VST3 validation

Build Steinberg's `validator` from the [VST3 SDK](https://github.com/steinbergmedia/vst3sdk)
(target `validator`), then:

```sh
validator "target/bundled/WHIP Send.vst3"
```

Current result: 47 tests passed, 0 failed.

## Known limitations

- **STUN/TURN:** ICE uses host candidates only. This is fine when the WHIP
  server is reachable directly (the usual case for a public Galène).
  `Link: rel="ice-server"` headers are logged but not used.
- **Worker thread priority:** the sender thread runs at normal priority. In a
  10-minute soak test on macOS, one system-wide scheduling hiccup of about
  200 ms was absorbed by sending silence and trimming once. Raising the thread
  priority (QoS class on macOS, `THREAD_PRIORITY_ABOVE_NORMAL` on Windows) is
  a candidate improvement.
- **Local macOS builds** are for the build machine's architecture (release
  builds are universal). For a local universal build, install both Rust
  targets and run `cargo xtask bundle-universal whip-send-plugin --release`.
- **Code signing:** the bundles are only ad-hoc signed, not notarized. Clear
  the quarantine flag after downloading (see *Download*).

## License

Copyright © 2026 Stefan Bethke.

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version. See [LICENSE](LICENSE).

The GPL is also required by nih-plug's VST3 bindings (`vst3-sys`), which are
GPLv3.
