# Plays the three audio tracks of live-test.mp4 in an endless loop into one
# multichannel audio device (for example VB-CABLE), to simulate a live
# production feed for Reaper. Track 1 goes to channels 1/2, track 2 to 3/4,
# track 3 to 5/6. Uses mpv, see README.md.
#
# usage: live-test\play.ps1 [-Device] DEVICE [-Channels 16|8] [-Shared]
#                           [-Track 1|2|3] [-File FILE]
#
# DEVICE is an mpv device ID (wasapi/{...}) or part of a device name, such as
# "CABLE In". Without DEVICE, lists the audio devices.

param(
    [Parameter(Position = 0)]
    [string]$Device,
    # The cable's channel count: 16, or 8 for a cable set to 7.1.
    [ValidateSet(16, 8)]
    [int]$Channels = 16,
    # Play through the Windows mixer instead of opening the device in WASAPI
    # exclusive mode. Doesn't work with the 16-channel VB-CABLE, whose shared
    # format has no speaker positions.
    [switch]$Shared,
    # Play only this track, on its usual channels, to check the mapping.
    [ValidateSet(0, 1, 2, 3)]
    [int]$Track = 0,
    [string]$File = $(if ($env:LIVE_TEST_FILE) { $env:LIVE_TEST_FILE } else { Join-Path $PSScriptRoot 'live-test.mp4' })
)

$ErrorActionPreference = 'Stop'

# mpv.com is the console version of mpv; mpv.exe prints nothing to the
# terminal. winget may not have put it on the PATH, so look in its package
# folder too.
function Find-Mpv {
    $cmd = Get-Command mpv.com -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $found = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet') -Recurse -Filter mpv.com -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($found) { return $found.FullName }
    throw 'mpv.com not found. Install mpv (winget install shinchiro.mpv) or put it on the PATH.'
}

# Returns the devices from "mpv --audio-device=help" as objects with Id and
# Name, e.g. 'wasapi/{...}' (CABLE Input (VB-Audio Virtual Cable)).
function Get-AudioDevices($mpv) {
    & $mpv --audio-device=help | ForEach-Object {
        if ($_ -match "^\s*'([^']+)'\s+\((.*)\)\s*$" -and $Matches[1] -ne 'auto') {
            [pscustomobject]@{ Id = $Matches[1]; Name = $Matches[2] }
        }
    }
}

$mpv = Find-Mpv
$devices = @(Get-AudioDevices $mpv)

if (-not $Device) {
    $devices | Format-Table Name, Id -AutoSize | Out-Host
    Write-Host "usage: $PSCommandPath DEVICE   (an ID, or part of a name, e.g. 'CABLE Input')"
    exit 0
}

$matched = @($devices | Where-Object { $_.Id -eq $Device })
if (-not $matched) {
    $matched = @($devices | Where-Object { $_.Name -like "*$Device*" })
}
if ($matched.Count -eq 0) {
    throw "no audio device matches '$Device'; run without arguments to list them"
}
if ($matched.Count -gt 1) {
    $matched | Format-Table Name, Id -AutoSize | Out-Host
    throw "'$Device' matches several devices; use more of the name, or the ID"
}
$target = $matched[0]

if (-not (Test-Path $File)) {
    throw "$File not found"
}

# Put the six channels unchanged on the first six of the cable, leaving the
# rest silent. The layout must match the cable's channel count, or Windows
# converts the audio and may move or mix channels. A 16-channel VB-CABLE
# reports no speaker positions, which mpv can't map, so name the first 16
# positions Windows knows (channel mask 0xFFFF). mpv and ffmpeg spell them
# differently.
if ($Channels -eq 16) {
    $layout = 'fl-fr-fc-lfe-bl-br-flc-frc-bc-sl-sr-tc-tfl-tfc-tfr-tbl'
    $pan = 'FL+FR+FC+LFE+BL+BR+FLC+FRC+BC+SL+SR+TC+TFL+TFC+TFR+TBL'
} else {
    $layout = '7.1'
    $pan = '7.1'
}
if ($Track -eq 0) {
    $graph = "[aid1][aid2][aid3]amerge=inputs=3,pan=$pan|c0=c0|c1=c1|c2=c2|c3=c3|c4=c4|c5=c5[ao]"
    $what = 'all tracks'
} else {
    $left = 2 * ($Track - 1)
    $graph = "[aid$Track]pan=$pan|c$left=c0|c$($left + 1)=c1[ao]"
    $what = "track $Track only, on channels $($left + 1)/$($left + 2)"
}
$mode = if ($Shared) { 'shared' } else { 'exclusive' }

Write-Host "Playing $what of $File to $($target.Name), $Channels channels, $mode mode."
Write-Host "Press q or Ctrl-C to quit. If mpv says 'Could not open/initialize audio device', it plays nothing."

# --loop-file loops the file; the outer loop restarts mpv if it fails, for
# example when the device goes away for a moment.
while ($true) {
    & $mpv --no-video --loop-file=inf "--audio-device=$($target.Id)" `
        "--audio-channels=$layout" --audio-samplerate=48000 `
        "--audio-exclusive=$(if ($Shared) { 'no' } else { 'yes' })" `
        "--lavfi-complex=$graph" $File
    # 0: quit with q; 4: quit by Ctrl-C or a signal.
    if ($LASTEXITCODE -eq 0 -or $LASTEXITCODE -eq 4) { break }
    Write-Warning "mpv stopped (exit code $LASTEXITCODE), restarting in 2 s"
    Start-Sleep -Seconds 2
}
