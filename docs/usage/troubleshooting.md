# Monitoring and troubleshooting

## Status in the editor

The status area shows the connection state, a peak meter of the sent signal
(after gain), and metrics from the last minute.

| State | Meaning |
|---|---|
| **LIVE** | Connected and sending. The time since connecting is shown. |
| **connecting** | WHIP request sent, waiting for ICE and DTLS. |
| **retrying** | The last attempt failed; the reason is shown below. The plugin tries again after 1, 2, 4, 5, 5 … seconds. |
| **disabled** | *Send enabled* is off. |
| **no endpoint** | No valid `http(s)://` endpoint has been applied. |

## Log file

All instances write their events, and a metrics line every minute, to one
shared log file. Lines are prefixed with the group name. The token is never
logged.

| OS | Log file |
|---|---|
| Windows | `%LOCALAPPDATA%\WhipSend\logs\whip-send.log` |
| macOS | `~/Library/Logs/WhipSend/whip-send.log` |
| Linux | `~/.local/state/whip-send/whip-send.log` |

At 10 MB the log is rotated to `whip-send.log.1`.

### Reading the metrics line

```
metrics state=live rate=48000 ch=1 block=128 cb_ms(n=22499 min=0.00 avg=2.67 max=8.71 sd=0.51)
  drift_ppm=-79.8 queue_max_ms=16.0 overrun_frames=0 silence_frames=0 trimmed_frames=0
  encoded=14994 pkts=14960 bytes=2123892 nacks=0 rtt_ms=7.6 loss=0.000 jitter_ms=2.2 lost=0 reconnects=0
```

(Wrapped here for readability; in the log it is one line.)

| Field | Meaning | What to look for |
|---|---|---|
| `rate`, `ch`, `block` | Host sample rate, channels sent, host block size. | |
| `cb_ms(...)` | Interval between the host's audio callbacks: count, min, average, max, standard deviation. | `avg` should equal block ÷ rate. A large `sd` or `max` means uneven delivery; Dante Virtual Soundcard problems show up here first. |
| `drift_ppm` | The host's effective sample rate against the computer's clock, since input last started. | Normally within ±100 ppm, and stable. A value that keeps growing means a clock problem. |
| `queue_max_ms` | Peak depth of the queue between the audio thread and the sender. | Normally about one block. |
| `overrun_frames` | Audio dropped because the queue was full. | Should stay 0. |
| `silence_frames` | 20 ms frames of silence sent while the host delivered nothing. | Expected while the transport is stopped or the track isn't processed. |
| `trimmed_frames` | Old audio discarded because the queue exceeded about 150 ms. | Occasional single events are harmless; a steady rise means the sender can't keep up. |
| `encoded`, `pkts`, `bytes` | Opus frames encoded, and RTP packets and bytes sent. | |
| `nacks` | Retransmission requests from the server. | |
| `rtt_ms`, `loss`, `jitter_ms`, `lost` | Round-trip time, and loss and jitter as reported by Galène. | Rising loss means a network problem between sender and server. |
| `reconnects` | Sessions re-established since the plugin started. | |

**Attributing a problem:**

- `drift_ppm` trending, or `cb_ms` deviation growing → clock or audio-device
  problem (see [one audio clock](live-operation.md#use-one-audio-clock)).
- `overrun_frames` or `trimmed_frames` rising → the sending thread is
  starved; the computer is overloaded.
- `loss` or `jitter_ms` rising → network between this computer and Galène.
- Everything clean, but listeners hear problems → check the listeners'
  `chrome://webrtc-internals` and the network after Galène.

## Common problems

| Symptom | Likely cause |
|---|---|
| `server returned HTTP 401` | No token sent, and the group requires one. |
| `server returned HTTP 404 (check the endpoint URL, group name and token)` | Wrong group name, or a wrong or expired token (Galène answers both with 404). |
| `server returned HTTP 403` | The token doesn't have the `present` permission. |
| `WHIP request failed: io: Connection refused` / timeouts | Server unreachable: wrong host or port, server down, or HTTP blocked. |
| Stuck in **connecting**, then `timed out establishing the connection` | The WHIP request worked, but UDP media can't get through. Check firewalls for outgoing UDP. |
| `connection lost (nothing received from the server for 5 s)` | The server went away or the network dropped. The plugin reconnects by itself. |
| `no input for 200 ms, sending silence` | Reaper stopped processing the track (transport stopped, monitoring off). |
| Peak meter shows *silence* while audio plays | Wrong channel mode (e.g. *Mono (right)* on a signal that is only on the left), or *Send gain* set very low. |
| The host crashes when you add the plugin or open its editor | No OpenGL 3.2, typically in a virtual machine. See below. |
| Reaper on macOS ignores typing in the endpoint and token fields | Use the **Paste** buttons. See [Gotchas](live-operation.md#gotchas). |

## Host crashes when adding the plugin

The editor needs OpenGL 3.2 or later. If the system doesn't offer it, the
editor fails while opening, and that takes the whole host down. On Windows,
Reaper shows *Microsoft Visual C++ Runtime Library: Runtime Error!* first,
and the Windows event log records an `APPCRASH` of `reaper.exe` with
exception code `40000015`.

This happens on computers without a working graphics driver, most often
virtual machines without 3D acceleration (Device Manager shows e.g.
*Microsoft Basic Display Adapter*, *Red Hat QXL* or *VirtIO GPU*). Windows
itself then only offers OpenGL 1.1.

**Fix on Windows:** install [Mesa for Windows](https://github.com/pal1000/mesa-dist-win/releases),
which provides OpenGL 4.5 in software. Download the `release-msvc` archive
and unpack it. Then either:

- **System-wide** (simplest, for a VM): run `systemwidedeploy.cmd` as
  administrator and choose the core desktop OpenGL drivers. Running the
  script again offers to uninstall.
- **For Reaper only:** copy `opengl32.dll`, `libgallium_wgl.dll` and, if
  present, `libglapi.dll` from the archive's `x64` folder into Reaper's
  folder, e.g. `C:\Program Files\REAPER (x64)`. Delete them to undo.

Restart Reaper afterwards. The editor is then drawn by the CPU, which is fine
for its simple interface. Don't install Mesa system-wide on a computer with a
real graphics card; fix its graphics driver instead.

### Finding the cause of other crashes

The plugin framework writes panics to a file if the `NIH_LOG` environment
variable names one. Start the host with it set, reproduce the crash, and
read the file. In PowerShell:

```powershell
$env:NIH_LOG = "$env:LOCALAPPDATA\WhipSend\logs\nih.log"
& "C:\Program Files\REAPER (x64)\reaper.exe"
```

On macOS and Linux, start the host from a terminal with `NIH_LOG=/tmp/nih.log`.
If `whip-send.log` has a `sender started` line, the plugin itself started and
the crash came later, e.g. when the editor opened.
