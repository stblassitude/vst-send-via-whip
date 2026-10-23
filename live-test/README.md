# Live test setup

Simulates a live production: a recording with three audio tracks plays in an
endless loop into a virtual audio device, and Reaper takes the three feeds
from there as live inputs and sends each one to its own Galène group.

```
live-test.mp4 ──player──▶ virtual device (6+ ch) ──▶ Reaper ──WHIP Send ×3──▶ Galène
  audio track 1                ch 1/2                  track 1                group 1
  audio track 2                ch 3/4                  track 2                group 2
  audio track 3                ch 5/6                  track 3                group 3
```

The audio tracks of the file are in group order: track 1 feeds the first
group, and so on. In the current recording these are English, German and
Portuguese.

There are three ways to set this up:

| Setup | OS | Player | Virtual device |
|---|---|---|---|
| [macOS](#macos-loopback-and-playsh) | macOS | `play.sh` (ffmpeg) | Loopback |
| [Windows](#windows-vb-cable-and-mpv) | Windows | `play.ps1` (mpv) | VB-CABLE |
| [Without a virtual device](#without-a-virtual-device) | any | Reaper itself | none |

The first two feed Reaper live inputs, like a real show: audio keeps flowing
whether or not the transport runs. The third is simpler but stops sending
audio whenever the transport stops.

`play.sh` is for macOS, `play.ps1` for Windows. `play.ps1` hasn't been tested yet.

## Requirements

- `live-test.mp4` in this directory. It isn't in the repository (see
  `.gitignore`); get it separately. It needs at least three audio tracks.
- A Galène server with three groups, e.g. `voc-a`, `voc-b` and `voc-c` from
  [`scripts/local-galene.sh`](../scripts/local-galene.sh).
- Reaper with WHIP Send installed.

## macOS: Loopback and play.sh

You need:

- [ffmpeg](https://ffmpeg.org/) (`brew install ffmpeg`);
- [Loopback](https://rogueamoeba.com/loopback/) by Rogue Amoeba, or another
  virtual audio device with at least 6 channels;
- optionally the Xcode command line tools (`xcode-select --install`), which
  give the script readable device names (see below).

### Create the virtual device in Loopback

1. Create a new virtual device and name it, e.g. *live-test*.
2. Remove all sources, including the default *Pass-Thru*, so only ffmpeg feeds
   it.
3. Set its channel count to **6** in the device's output channels.

### Start playback

List the output devices and their indices:

```sh
live-test/play.sh
```

```
index  channels  name
    6         2  MacBook Pro-Lautsprecher
   13         6  live-test
```

Then start playback on the Loopback device:

```sh
live-test/play.sh 13
```

ffmpeg plays the file in a loop at real-time speed and shows its progress.
`speed=` should stay around `1x`. If ffmpeg stops, the script restarts it after
two seconds. Press **Ctrl-C** to quit. To play another file, set
`LIVE_TEST_FILE=/path/to/file.mp4`.

> **Note:** Device indices change when devices are added or removed, so list
> them again after plugging something in. Without the Xcode command line
> tools the script falls back to ffmpeg's own list, which shows Loopback
> devices as `(null)` and includes input devices. Look for the
> `com.rogueamoeba.Loopback` entries.

Continue with [Configure Reaper](#configure-reaper), choosing *CoreAudio* and
the Loopback device.

## Windows: VB-CABLE and mpv

ffmpeg can't choose an output device on Windows, so this setup uses
[mpv](https://mpv.io/) as the player. It decodes and filters with the same
libraries as ffmpeg, and can play to any device.

You need:

- [VB-CABLE](https://vb-audio.com/Cable/) by VB-Audio (free). It installs one
  virtual device with two ends: *CABLE Input* for playback and *CABLE Output*
  for recording.
- [mpv](https://mpv.io/installation/), e.g. `winget install shinchiro.mpv`
  or `scoop install mpv`.

### Set up VB-CABLE

Use one cable with at least 6 channels, not several stereo cables. Reaper can
open only one input device at a time.

1. In the Windows sound control panel (*Sound → Playback*), open the
   properties of *CABLE Input*. On the *Advanced* tab, choose a format with
   **48000 Hz**. Do the same for *CABLE Output* on the *Recording* tab.
2. Note the channel count the format shows, e.g. *16 channel, 24 bit,
   48000 Hz*:
    - **16 channels:** nothing else to set up. *Configure* is greyed out,
      because Windows has no speaker layout for 16 channels. The channels
      are passed through as they are.
    - **8 channels or fewer:** select *CABLE Input*, click *Configure* and
      choose **7.1 Surround**, so the cable carries 8 channels.

### Start playback

In PowerShell, list the audio devices:

```powershell
live-test\play.ps1
```

Then start playback on the cable. The device can be its ID or part of its
name:

```powershell
live-test\play.ps1 'CABLE Input'
```

For an 8-channel cable, add `-Channels 8`. To play another file, add
`-File path\to\file.mp4`.

The script finds `mpv.com` on the PATH or in winget's package folder. It puts
the three tracks unchanged on the first six channels of the cable, and leaves
the rest silent. The channel count must match the cable's, otherwise Windows
converts the audio and may move or mix channels. If mpv stops, the script
restarts it after two seconds. Press **q** or **Ctrl-C** to quit.

> **Note:** If PowerShell refuses to run the script because running scripts
> is disabled, allow it once with
> `powershell -ExecutionPolicy Bypass -File live-test\play.ps1 'CABLE Input'`,
> or for good with `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`.

**Check the channel mapping once.** Play one track at a time and check in
Reaper that only the expected inputs move. For example, this plays only the
second track, which should arrive on inputs 3/4. Use the device ID from the
list, `mpv` from the PATH or the full path to `mpv.com`, and for an 8-channel
cable `--audio-channels=7.1` and `pan=7.1`:

```powershell
mpv --no-video --audio-device='wasapi/{0.0.0.00000000}.{…}' `
    --audio-channels=16 --audio-samplerate=48000 `
    --lavfi-complex='[aid2]pan=16c|c2=c0|c3=c1[ao]' `
    live-test\live-test.mp4
```

This matters most for an 8-channel cable: channel 4 is the LFE channel of the
7.1 layout, and some drivers filter it.

Continue with [Configure Reaper](#configure-reaper), choosing *WASAPI* and
*CABLE Output*.

## Configure Reaper

1. **Audio device:** *Preferences → Audio → Device*. Set the input device and
   the sample rate to **48000** Hz:
    - **macOS:** *CoreAudio*, input device: the Loopback device (*live-test*);
    - **Windows:** *WASAPI*, input device: *CABLE Output (VB-Audio Virtual
      Cable)*.

    The output device doesn't matter. Use your speakers or headphones if you
    want to listen.
2. **Disable anticipative FX processing:** *Preferences → Audio → Buffering*,
   untick *Anticipative FX processing*, or do it per track under *Track →
   Performance options*.
3. **Create three tracks,** one per feed, with these inputs (*Input:
   Stereo*):

    | Track | Input | Galène group (example) |
    |---|---|---|
    | 1 | 1/2 | `voc-a` |
    | 2 | 3/4 | `voc-b` |
    | 3 | 5/6 | `voc-c` |

4. On each track, **arm the track and turn input monitoring on.** To avoid
   filling the disk, set the record mode to *Record: disable (input
   monitoring only)* (right-click the record-arm button). The meters should
   now move, whether or not the transport is running.
5. **Insert WHIP Send** as an FX on each track and enter:
    - **Endpoint:** the group URL with `.whip` appended, e.g.
      `http://127.0.0.1:8444/group/voc-a/.whip` for the local test server;
    - **Token:** a token for that group, or nothing if the group lets
      anyone present;
    - **Content:** *Voice*.

    Typing into these fields doesn't work in Reaper on macOS. Copy the
    value, then click **Paste** next to the field. Then click **Apply**.
    The status should change to *LIVE*.
6. Open the groups in a browser (e.g. `http://127.0.0.1:8444/group/voc-a/`)
   to listen.

Save the project so you can reopen it later. On macOS, only the Loopback
device index for `play.sh` may change between runs.

## Without a virtual device

Reaper can also play the recording itself. This works on any OS and needs no
extra software besides ffmpeg. It is less realistic: the plugins only get
audio while the transport runs, and they send silence when it stops.

1. Extract the three audio tracks as WAV files, from the `live-test`
   directory:

    ```sh
    ffmpeg -i live-test.mp4 \
        -map 0:a:0 -c:a pcm_s24le -ar 48000 track1.wav \
        -map 0:a:1 -c:a pcm_s24le -ar 48000 track2.wav \
        -map 0:a:2 -c:a pcm_s24le -ar 48000 track3.wav
    ```

    On Windows, write the command on one line or replace `\` with `^`.
2. In Reaper, set the project sample rate to 48000 Hz and create three
   tracks. Insert `track1.wav` on track 1, `track2.wav` on track 2 and
   `track3.wav` on track 3, all starting at the same position.
3. Select the three items and run the action *Loop points: Set loop points to
   selected items* (*Actions → Show action list*). Turn on **Repeat** (**R**).
4. Do steps 2, 5 and 6 of [Configure Reaper](#configure-reaper):
   disable anticipative FX processing (more important here, since Reaper
   otherwise renders playing tracks ahead of time) and insert WHIP Send on
   each track.
5. Start playback. Keep the transport running for as long as the test runs.

## Notes

- Routing through a virtual device is fine here: it is Reaper's only audio
  device, so Reaper still sees a single clock. The player paces the file by
  the system clock. Over long runs that may cause an occasional small glitch
  at the virtual device, which doesn't happen with a real audio interface.
  For a proper soak test, use the real hardware (see
  [Live operation](../docs/usage/live-operation.md)).
- Real shows on Windows should use an ASIO driver. VB-CABLE has none, but
  WASAPI is fine for this test.
- There may be a tiny gap where the file loops (after about 57 minutes for
  the current recording).
