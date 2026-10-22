# Testing

## Unit tests

```sh
cargo test --workspace
```

The tests cover:

- URL resolution of the WHIP `Location` header;
- Opus framing;
- resampling ratios;
- the drift estimator, with simulated host clocks and jittery observation;
- network interface selection;
- settings handling, including that the token is never printed.

## Local Galène

`scripts/local-galene.sh` builds Galène from a source checkout and runs it
with plain HTTP on port 8444:

```sh
git clone https://github.com/jech/galene.git ~/src/galene
scripts/local-galene.sh ~/src/galene        # work directory defaults to $TMPDIR/whip-send-galene
```

| Group | Access |
|---|---|
| `open`, `voc-a`, `voc-b`, `voc-c` | Anyone may present; no token needed. |
| `secure` | Presenting needs a token. |

The admin user is `admin` with password `admin`. Create a token for `secure`
with:

```sh
W=${TMPDIR:-/tmp}/whip-send-galene
$W/galenectl -config $W/galenectl.json -server http://127.0.0.1:8444 \
    -admin-username admin -admin-password admin \
    create-token -group secure -permissions present
```

Listen at `http://127.0.0.1:8444/group/voc-a/` (log in as any user).

## Test sender

`whip-send-cli` simulates a plugin host: it feeds a sine tone through the same
code the plugin uses. You can set the host's sample rate, block size, clock
error and stalls, so situations like a drifting Dante Virtual Soundcard can be
reproduced without a DAW.

```sh
E=http://127.0.0.1:8444/group
cargo run --release -p whip-send-cli -- --endpoint $E/voc-a/.whip --seconds 60
cargo run --release -p whip-send-cli -- --endpoint $E/voc-b/.whip --rate 44100 --block 1024 --drift-ppm 150 --mode sum
cargo run --release -p whip-send-cli -- --endpoint $E/voc-c/.whip --block 128 --drift-ppm -80 --stall-at 20 --stall-for 3
```

| Option | Meaning |
|---|---|
| `--endpoint URL` | WHIP endpoint. |
| `--token TOKEN` | Bearer token; can also be set in `WHIP_TOKEN`. |
| `--seconds N` | Run time; 0 = until killed. Default 30. |
| `--rate HZ`, `--block FRAMES` | Simulated host sample rate and block size. |
| `--drift-ppm PPM` | Simulated host clock error. |
| `--mode MODE` | `left`, `right`, `sum` or `stereo`. |
| `--bitrate BPS`, `--music` | Encoder settings. |
| `--freq HZ` | Tone frequency of the left channel; the right channel is 1.5×. |
| `--stall-at S --stall-for S` | Stop delivering audio for a while. |
| `--metrics S` | Metrics interval; default 10 s. |
| `--log-file` | Also write to the shared log file. |

The exit code is 0 if any packets were sent.

### Things to verify

These tests were run during development; repeat them after changes to
`whip-core`.

| Scenario | How | Expected |
|---|---|---|
| Drift measurement | Three streams with `--drift-ppm 0`, `150`, `-80`, 10 min | `drift_ppm` within a few ppm of the setting. |
| Token handling | `secure` group without a token, with a wrong one, and with a valid one | 401, 404, then live. |
| Input stall | `--stall-at 8 --stall-for 3` | Silence after 200 ms, about 140 silence frames, clean recovery. |
| Server restart | Kill Galène during a run, restart it a few seconds later | `nothing received … for 5 s`, retries, back live. |

## Standalone plugin

The standalone binary runs the real plugin wrapper without a DAW. The
endpoint can be preset from the environment:

```sh
cargo build --release -p whip-send-plugin --bin whip-send-standalone
WHIP_ENDPOINT=http://127.0.0.1:8444/group/voc-a/.whip \
  ./target/release/whip-send-standalone -b core-audio --input-device "<device>"
```

- `WHIP_TOKEN` sets the token.
- `WHIP_HEADLESS=1` runs without the editor window.
- `-b dummy` feeds silence without touching audio devices, useful in CI-like
  environments.
- `--help` lists the audio options.
- On macOS, using a CoreAudio input requires microphone permission for the
  terminal.

## VST3 validation

Build Steinberg's `validator` from the
[VST3 SDK](https://github.com/steinbergmedia/vst3sdk):

```sh
git clone --depth 1 https://github.com/steinbergmedia/vst3sdk.git
cd vst3sdk
git submodule update --init --depth 1 base pluginterfaces public.sdk cmake
cmake -S . -B build -DSMTG_ENABLE_VSTGUI_SUPPORT=OFF -DSMTG_ADD_VST3_PLUGINS_SAMPLES=OFF
cmake --build build --target validator --config Release
```

Then:

```sh
build/bin/Release/validator "path/to/WHIP Send.vst3"
```

Current result: 47 tests passed, 0 failed.

## Listening tests

In Chrome, open the Galène group page, then `chrome://webrtc-internals`, and
watch the inbound audio track: `concealedSamples`, `jitterBufferDelay`,
`insertedSamplesForDeceleration` and `removedSamplesForAcceleration`. See
[Live operation](../usage/live-operation.md#before-the-event).
