# Live operation and tips

These recommendations come from a previous event. There, a standalone ingest
tool fed from a Dante Virtual Soundcard degraded over 30–45 minutes until it
went silent ([background](../architecture/audio-path.md#lessons-from-listen-ingest)).
WHIP Send is designed to avoid that failure. A sound setup still matters.

## Use one audio clock

**Reaper's audio device should be the only clock in the chain.** The plugin
receives audio on that device's clock and timestamps the stream from the
sample count. Everything downstream (Galène and the listeners' browsers)
follows that clock without trouble.

Problems start when two clocks meet in a buffer somewhere: a virtual cable,
a loopback device, or an aggregate of two interfaces.

- **Don't** route audio through virtual cables or loopback devices (VB-Cable,
  Voicemeeter, BlackHole, Loopback) on the way to the plugin.
- **Don't** combine two audio interfaces in Reaper.

### Dante

- **Preferred: a hardware Dante interface,** such as a PCIe card, a
  Dante-to-USB adapter, or an interface with Dante. It locks its sample clock
  to the network in hardware, so the computer sees an ordinary single-clock
  device.
- **If you have to use Dante Virtual Soundcard (DVS):**
    - make it Reaper's **only** audio device, through its **ASIO** driver
      (on Windows), not WDM/WASAPI;
    - use a wired, onboard network interface, not Wi-Fi or a USB adapter, with
      Energy-Efficient Ethernet and NIC power saving switched off;
    - set a moderate DVS latency of 4–6 ms or more;
    - check the clock sync status in Dante Controller.

## Reaper settings

- **Disable anticipative FX processing** for the sending tracks (*Track →
  Performance options*), or globally (*Preferences → Audio → Buffering*).
  Otherwise Reaper may process these tracks ahead of time in bursts.
- **Keep the tracks processing.** The plugin only receives audio while Reaper
  processes the track, for example when it is armed with input monitoring.
  If audio stops, the plugin sends silence after 200 ms so the session stays
  up. It doesn't reconnect.
- **Offline renders are never streamed.** Rendering a project with the plugin
  in it is safe.
- The plugin reports **zero latency**, so plugin delay compensation is
  unaffected.

## Before the event

1. **Soak test the real setup** for at least 2–3 hours: the same computer,
   audio device, Dante configuration and Galène server, with all feeds
   running. A USB interface on the bench doesn't exercise Dante problems.
2. **Listen in Chrome** and open `chrome://webrtc-internals`. On the inbound
   audio track:
    - `concealedSamples` should stay near zero;
    - `jitterBufferDelay` should stay flat;
    - `insertedSamplesForDeceleration` and `removedSamplesForAcceleration`
      show how much drift correction the browser applies.
3. **Check the plugin's log** for trends. See
   [Monitoring and troubleshooting](troubleshooting.md).
4. **Check token lifetimes.** Galène tokens expire, so create them to last
   beyond the event.

## Network

- The plugin connects to the server directly, using the computer's own
  addresses. STUN/TURN servers are not used yet.
- This works whenever the Galène server is reachable from the sending
  computer. That is the normal case for a public Galène, including from
  behind NAT.
- It does **not** work through a firewall that blocks outgoing UDP.
- For the WHIP request itself, the endpoint's port must be reachable over
  HTTP(S). By default that is 8443 for Galène.

## Gotchas

- **Typing in the endpoint or token field changes nothing** until you click
  **Apply**. *unsaved changes* next to the buttons means there are edits
  pending.
- **A wrong token shows as HTTP 404**, not 403: Galène doesn't reveal whether
  the group exists. The status line then says to check the URL, group name
  and token.
- **Mono modes** pick the left channel, the right channel, or their average.
  On a stereo track carrying two different sources, choose deliberately.
- **Several instances in one project** each open their own connection. Three
  feeds need three groups (or three tokens), and each feed has its own status.
