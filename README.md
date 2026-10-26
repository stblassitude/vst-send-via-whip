# WHIP Send

[![CI](https://github.com/stblassitude/vst-send-via-whip/actions/workflows/ci.yml/badge.svg)](https://github.com/stblassitude/vst-send-via-whip/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/stblassitude/vst-send-via-whip)](https://github.com/stblassitude/vst-send-via-whip/releases/latest)
[![Docs](https://img.shields.io/badge/docs-online-blue)](https://stblassitude.github.io/vst-send-via-whip/)

An audio plugin (VST3 and CLAP) that publishes a track's audio to a WHIP
endpoint, such as a [Galène](https://galene.org/) group. The track's audio
passes through unchanged. One plugin instance sends one stream, so three
instances on three tracks feed three Galène groups.

**Documentation: <https://stblassitude.github.io/vst-send-via-whip/>**
(source in [`docs/`](docs/)).

## Highlights

- **Platforms:** VST3 and CLAP for Windows, macOS (universal) and Linux.
- **Audio:** mono (left, right or L+R) or stereo, sent as Opus at 48 kHz,
  with resampling as needed.
- **Settings:** endpoint, token and encoder settings are stored in the
  project and in FX presets. *Send enabled*, *Send gain* and *Channels* can
  be automated.
- **Real-time safe:** the audio thread never blocks or allocates, and the
  plugin adds no latency to the track.
- **Recovers by itself:** a lost connection is detected within about 5 s,
  and the plugin reconnects with backoff.
- **Metrics:** host clock drift, callback timing, queue depth, loss, jitter
  and RTT are shown in the editor and logged every minute.

## Quick start

1. Download the archive for your platform from the
   [latest release](https://github.com/stblassitude/vst-send-via-whip/releases/latest),
   and copy `WHIP Send.vst3` or `WHIP Send.clap` into your plugin folder.
   On macOS, run `xattr -dr com.apple.quarantine` on the copied bundle.
2. In Galène, create a token with the `present` permission:
   `galenectl create-token -group voc-a -permissions present`.
3. In Reaper, insert *WHIP Send* on the track and enter:
    - the endpoint, e.g. `https://galene.example.org:8443/group/voc-a/.whip`;
    - the token.

    Then click **Apply**.

Before using it at an event, read
[Live operation and tips](https://stblassitude.github.io/vst-send-via-whip/usage/live-operation/).
It covers the audio-clock setup (especially with Dante), Reaper settings and
soak testing.

> **Note:** project files contain the token (encoded, not encrypted). Treat
> them as secrets.

## Building

Requirements: a recent stable Rust and CMake. Linux also needs the
X11/OpenGL/ALSA development packages listed in the documentation.

```sh
cargo xtask bundle whip-send-plugin --release   # → target/bundled/WHIP Send.{vst3,clap}
cargo test --workspace
```

See [Building](https://stblassitude.github.io/vst-send-via-whip/development/building/)
and [Testing](https://stblassitude.github.io/vst-send-via-whip/development/testing/).
Releases are built by GitHub Actions when a `vX.Y.Z` tag is pushed. The tag
sets the plugin version.

> **Note:** ureq's own TLS connector sends no ALPN, which some WHIP servers
> require. [`tls.rs`](crates/whip-core/src/tls.rs) therefore supplies one
> that offers `http/1.1`, built on ureq's `unversioned::transport` API. That
> API may change in minor ureq releases, so check this file first if a ureq
> update breaks the build.

## License

Copyright © 2026 Stefan Bethke. Licensed under the GNU General Public License,
version 3 or later; see [LICENSE](LICENSE). The GPL is also required by
nih-plug's VST3 bindings.
