#!/bin/sh
# Plays the three audio tracks of live-test.mp4 in an endless loop into one
# 6-channel audio device (for example a Loopback virtual device), to simulate
# a live production feed for Reaper. Track 1 goes to channels 1/2, track 2 to
# 3/4, track 3 to 5/6. macOS only.
#
# usage: live-test/play.sh [DEVICE_INDEX]
#
# Without DEVICE_INDEX, lists the output devices and their indices.

set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "$0: only macOS is supported" >&2
    exit 1
fi

dir=$(cd "$(dirname "$0")" && pwd)
input=${LIVE_TEST_FILE:-$dir/live-test.mp4}

# ffmpeg's own device list shows "(null)" for some virtual devices, Loopback's
# among them, so ask Core Audio for the names. The order is the same as
# ffmpeg's -audio_device_index.
list_devices() {
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    cat > "$tmp/list.swift" <<'EOF'
import CoreAudio

func address(_ sel: AudioObjectPropertySelector, _ scope: AudioObjectPropertyScope) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: sel, mScope: scope, mElement: kAudioObjectPropertyElementMain)
}

let system = AudioObjectID(kAudioObjectSystemObject)
var addr = address(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal)
var size: UInt32 = 0
AudioObjectGetPropertyDataSize(system, &addr, 0, nil, &size)
var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
AudioObjectGetPropertyData(system, &addr, 0, nil, &size, &ids)

print("index  channels  name")
for (index, id) in ids.enumerated() {
    var a = address(kAudioDevicePropertyStreamConfiguration, kAudioObjectPropertyScopeOutput)
    var s: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(id, &a, 0, nil, &s) == noErr, s > 0 else { continue }
    let buffers = UnsafeMutableRawPointer.allocate(byteCount: Int(s), alignment: 16)
    defer { buffers.deallocate() }
    guard AudioObjectGetPropertyData(id, &a, 0, nil, &s, buffers) == noErr else { continue }
    let channels = UnsafeMutableAudioBufferListPointer(buffers.assumingMemoryBound(to: AudioBufferList.self))
        .reduce(0) { $0 + Int($1.mNumberChannels) }
    guard channels > 0 else { continue }

    var n = address(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal)
    var name: CFString = "?" as CFString
    var ns = UInt32(MemoryLayout<CFString>.size)
    _ = withUnsafeMutablePointer(to: &name) { AudioObjectGetPropertyData(id, &n, 0, nil, &ns, $0) }
    print(String(format: "%5d  %8d  %@", index, channels, name as String))
}
EOF
    if command -v swift >/dev/null 2>&1 && swift "$tmp/list.swift"; then
        :
    else
        # No Swift toolchain: fall back to ffmpeg's list (inputs included).
        ffmpeg -hide_banner -f lavfi -i anullsrc -t 0.01 \
            -f audiotoolbox -list_devices true - 2>&1 | grep '\] \[[0-9]*\]'
    fi
    echo
    echo "usage: $0 DEVICE_INDEX   (pick a device with 6 channels)"
}

if [ $# -eq 0 ]; then
    list_devices
    exit 0
fi

device=$1
case $device in
    ''|*[!0-9]*) echo "$0: DEVICE_INDEX must be a number" >&2; exit 2 ;;
esac

if [ ! -f "$input" ]; then
    echo "$0: $input not found" >&2
    exit 1
fi

# Ctrl-C ends the script instead of restarting ffmpeg.
trap 'exit 0' INT TERM

# -stream_loop loops the file; the outer loop restarts ffmpeg if it fails,
# for example when the device goes away for a moment.
while :; do
    ffmpeg -hide_banner -loglevel warning -stats \
        -re -stream_loop -1 -i "$input" \
        -filter_complex '[0:a:0][0:a:1][0:a:2]amerge=inputs=3,aresample=48000[a]' \
        -map '[a]' -c:a pcm_f32le \
        -f audiotoolbox -audio_device_index "$device" - || true
    echo "ffmpeg stopped, restarting in 2 s (Ctrl-C to quit)" >&2
    sleep 2
done
