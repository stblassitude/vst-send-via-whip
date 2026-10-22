# WHIP Send

WHIP Send is an audio plugin (VST3 and CLAP) that publishes a track's audio to
a [WHIP](https://www.rfc-editor.org/rfc/rfc9725) endpoint, such as a
[Galène](https://galene.org/) group. The track's audio passes through
unchanged.

It was written for live events where several audio feeds from a Reaper
session (for example the floor and interpreter channels) must reach an
audience through Galène. One plugin instance sends one stream, so three
instances on three tracks feed three Galène groups.

## Features

- **Formats:** VST3 and CLAP for Windows, macOS (universal) and Linux.
- **Audio:** mono (left, right or L+R) or stereo, sent as Opus at 48 kHz.
  Projects at other sample rates are resampled automatically.
- **Settings:** endpoint, token and encoder settings are stored in the
  project file and in FX presets.
- **Automation:** *Send enabled*, *Send gain* and *Channels* can be
  automated or driven from actions and OSC.
- **Real-time safe:** the audio thread never blocks or allocates, and the
  plugin adds no latency to the track.
- **Recovers by itself:** a lost connection is detected within about 5 s,
  and the plugin reconnects with backoff.
- **Built-in metrics:** host clock drift, callback timing, queue depth, loss,
  jitter and RTT are shown in the editor and logged once a minute. Problems
  at an event can be diagnosed afterwards.

## Where to start

- **Operators:** [Installation](usage/installation.md),
  [Configuration](usage/configuration.md), and especially
  [Live operation and tips](usage/live-operation.md).
- **Developers:** [Building](development/building.md) and
  [Testing](development/testing.md).
- **Why it works the way it does:** [Architecture](architecture/index.md).

## License

WHIP Send is free software under the
[GNU General Public License, version 3 or later](https://github.com/stblassitude/vst-send-via-whip/blob/main/LICENSE).
