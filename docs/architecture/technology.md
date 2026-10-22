# Technology choices

## Approaches considered

Reaper can't send WebRTC itself. No VST plugin that publishes to WHIP was
found, only proprietary streaming plugins with their own protocols. The
options were:

1. **A custom plugin that speaks WHIP.** This is the most integrated: one
   plugin per track, configured and saved in Reaper.
2. **ReaStream to a local helper process.** ReaStream sends each track over
   UDP on localhost, and a helper keeps the WHIP sessions.
3. **Routing audio out of Reaper** (ReaRoute ASIO, virtual cables) into an
   existing WHIP client such as GStreamer, OBS or FFmpeg.

Option 3 brings a second clock into the chain, the failure mode described in
[Audio path and clocking](audio-path.md). Option 2 is a good fallback.
WHIP Send is option 1. `whip-core` is independent of the plugin, so a
ReaStream helper could reuse it.

## Plugin formats

| Format | Reaper support | Decision |
|---|---|---|
| VST3 | Full | **Yes.** The universal choice. |
| CLAP | Full (since 6.71) | **Yes.** It comes free with the same code. |
| VST2 | Full | No; the SDK is no longer distributed. |
| AU, LV2 | macOS only / Linux only | Not needed for Reaper. |

## Language and libraries

Rust was chosen over C++ mainly for memory safety, since the plugin runs
inside Reaper during a live show. It also builds a self-contained binary with
one `cargo` command.

| Concern | Library | Why |
|---|---|---|
| Plugin framework | [nih-plug](https://github.com/robbert-vdh/nih-plug) | VST3 and CLAP from one crate on Windows, macOS and Linux. It supports persisted non-parameter state, which is used for endpoint and token. |
| GUI | `nih_plug_egui` ([egui](https://github.com/emilk/egui)) | Simple immediate-mode UI: text fields, status and meter. |
| WebRTC | [str0m](https://github.com/algesten/str0m) | Sans-IO and synchronous: the worker owns the sockets and the loop, so there is no async runtime inside the DAW. It handles Opus packetization, RTCP and ICE. Its pure-Rust crypto backend avoids OpenSSL. |
| Opus | [libopus](https://opus-codec.org/) via the `opus` crate | The reference encoder. It is built with CMake and linked statically. |
| Resampling | [rubato](https://github.com/HEnquist/rubato) | Pure Rust; a sinc resampler for projects not running at 48 kHz. |
| HTTP | [ureq](https://github.com/algesten/ureq) with rustls | Small, blocking, no OpenSSL. WHIP needs only POST and DELETE. |
| Audio hand-over | [rtrb](https://github.com/mgeier/rtrb) | Lock-free, wait-free single-producer single-consumer ring buffer. |
| Interfaces | [if-addrs](https://github.com/messense/if-addrs) | Lists local addresses for ICE host candidates. |

The bundles link only system libraries: no OpenSSL, no Opus DLL, and nothing
to install next to the plugin. That matters, because DLLs placed next to a
plugin are not on Windows' DLL search path.

## Alternatives considered

- **C++ with JUCE (or CLAP SDK + clap-wrapper) and libdatachannel.**
  libdatachannel is what OBS Studio's WHIP output uses, so it is well proven
  against WHIP servers. It was rejected because of C++ memory-safety risk
  inside the host, and more involved static linking on Windows. If str0m ever
  causes problems with Galène or difficult NATs, it is the fallback for the
  WebRTC layer, either through FFI or with a C++ rewrite.
- **webrtc-rs.** A more complete port of Pion, but it needs a Tokio runtime
  in the host process, and is harder to tear down cleanly when the plugin is
  unloaded.
- **Google libwebrtc.** Huge, hard to build, and far more than one Opus track
  needs.

## Licensing

nih-plug's VST3 bindings (`vst3-sys`) are GPLv3, so VST3 builds must be
distributed under GPL-compatible terms. WHIP Send is licensed GPL-3.0-or-later.
