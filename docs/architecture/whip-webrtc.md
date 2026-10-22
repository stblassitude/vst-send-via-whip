# WHIP and WebRTC

## Session setup

WHIP ([RFC 9725](https://www.rfc-editor.org/rfc/rfc9725)) is a single HTTP
exchange that sets up a WebRTC session. Each attempt works as follows:

1. **Sockets.** Bind one non-blocking UDP socket for each usable local
   address, and add each as an ICE host candidate. Loopback, link-local and
   multicast addresses are skipped. Loopback is used only when the endpoint
   itself is on this machine.
2. **Offer.** Create a str0m `Rtc` with a single **send-only** audio media
   and Opus as the only codec, and generate the SDP offer. All candidates are
   already in the offer (no trickle ICE).
3. **POST** the offer to the endpoint on a helper thread, with
   `Content-Type: application/sdp` and `Authorization: Bearer <token>` (if a
   token is set). The timeout is 5 s.
4. **Answer.** Expect `201 Created` (or 200) with an SDP answer:
    - store the `Location` header, resolved against the endpoint URL, and the
      `ETag`;
    - pass the answer to str0m.
5. **Connect.** ICE and DTLS complete within 15 s. Once str0m reports
   `Connected`, the session is **live** and encoded frames are written to it.
6. **End.** On teardown, `DELETE` the session URL (with `If-Match` when an
   ETag was given) on a helper thread, with a 2 s timeout.

If a session is abandoned while its POST is still in flight (settings
changed, plugin disabled), the POST thread deletes the session as soon as
the answer arrives. This avoids orphaned streams on the server.

### SDP details

| Item | Value |
|---|---|
| Codec | Opus, payload type 111, `opus/48000/2` |
| fmtp | `minptime=10; useinbandfec=1; stereo=1; sprop-stereo=1` |
| Feedback | `nack` and `transport-cc` offered |
| Direction | `sendonly` |

`stereo=1` is always offered, so switching between mono and stereo modes
needs no renegotiation; Opus decoders handle mono packets in a stereo
session.

## Galène specifics

- **Endpoint:** `https://host[:port]/group/<name>/.whip`.
- **Authentication:** bearer token only. The token needs the `present`
  permission.
- **Response codes:**
    - 401: no token, and one is required;
    - 404: unknown group, **or** a wrong or expired token;
    - 403: the token can't present.
- **ICE servers:** Galène announces them in `Link` headers. WHIP Send logs
  them but doesn't use them yet (see [Limitations](#limitations)).
- **Codecs:** the group must allow Opus (the default).

## Opus settings

| Setting | Voice | Music |
|---|---|---|
| Application | `VOIP` | `AUDIO` |
| Signal hint | voice | music |
| Bitrate | as configured, 16–256 kbit/s (default 64) | same |
| Frame size | 20 ms | 20 ms |
| VBR | on | on |
| In-band FEC | on, with a 10 % expected-loss setting | same |
| DTX | off | off |
| Complexity | 10 | 10 |

Three streams at complexity 10 cost a few percent of one CPU core.

## Keeping the session healthy

- **RTCP.** str0m sends sender reports and processes Galène's receiver
  reports. The resulting loss, jitter, RTT and packet counts are surfaced
  through egress statistics every second.
- **Liveness.** str0m only declares ICE disconnected after its consent
  timeout, about 30 s. That's too slow for a live show, so WHIP Send
  considers a live session lost if **nothing at all** (STUN consent checks,
  RTCP) has been received from the server for **5 s**. Galène keeps sending
  receiver reports and consent checks on a healthy session, and no false
  triggers occurred in soak tests. In a test, a killed Galène was detected
  within 5 s, and the stream was live again within a few seconds of the
  server coming back.
- **Errors** end the session: a failed WHIP request, an invalid answer, a
  connect timeout, ICE disconnection, the liveness timeout, or socket
  errors.

## Reconnecting

After a failure, the worker waits and tries again. The delay doubles from
1 s up to 5 s (1, 2, 4, 5, 5 …). It resets after a session has stayed up for
30 s, after a manual **Reconnect**, or when the endpoint or token change.

A new attempt is always a fresh POST with a new peer connection. ICE restarts
via `PATCH` are not used: they are more complex, and a new session is just as
quick with Galène.

Audio keeps draining while not live, so the queue doesn't fill up and nothing
stale is sent once the new session is up. RTP timestamps start at a random
value for each session.

| Event | Effect |
|---|---|
| Endpoint or token applied in the editor | End the session, connect immediately with the new settings. |
| Bitrate or content changed | Re-create the encoder; the session stays up. |
| *Send enabled* off | End the session (DELETE). |
| *Send enabled* on | Connect. |
| Plugin removed or project closed | End the session, join all threads. |

## Logging

Events (connecting, live, errors, stalls, trims, settings changes) and the
minute-by-minute metrics go to `log` and to a shared, size-rotated log file.
Lines carry a UTC timestamp and the stream's label: the Galène group name,
taken from the endpoint.

The token never appears in logs, status text or debug output.
`StreamSettings`'s `Debug` implementation redacts it, and endpoints are logged
only with "(with token)" or "(no token)".

## Limitations

- **STUN/TURN.** Only host candidates are used. This is fine when the WHIP
  server is directly reachable, which is the usual case for a public Galène,
  since it learns the sender's public address from incoming checks. It fails
  when outgoing UDP is blocked. Supporting the `Link` ICE servers (STUN, and
  TURN via a relay client) would remove this restriction.
- **Worker priority.** The worker runs at normal thread priority. In a
  10-minute macOS soak test, one system-wide 200 ms scheduling hiccup was
  absorbed by silence and a single trim. A higher priority (macOS QoS class,
  `THREAD_PRIORITY_ABOVE_NORMAL` on Windows) is a candidate improvement.
