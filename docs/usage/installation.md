# Installation

## Download

Each [release](https://github.com/stblassitude/vst-send-via-whip/releases) has
one archive per platform:

| Archive | Platform |
|---|---|
| `whip-send-<version>-windows-x64.zip` | Windows 10/11, x64 |
| `whip-send-<version>-macos-universal.zip` | macOS 11 or later, Apple Silicon and Intel |
| `whip-send-<version>-linux-x64.zip` | Linux x64 (glibc 2.35 or later) |

Each archive contains:

- `WHIP Send.vst3`, the VST3 plugin;
- `WHIP Send.clap`, the CLAP plugin;
- `whip-send-cli`, a test sender that publishes a tone (see
  [Testing](../development/testing.md#test-sender));
- `README.md` and `LICENSE`.

Use either the VST3 or the CLAP plugin. Both behave the same.

## Requirements

The plugin's editor draws with **OpenGL 3.2 or later**. Any computer with a
working graphics driver has that. Virtual machines often don't: without 3D
acceleration, Windows only offers OpenGL 1.1, and the host **crashes as soon
as the editor opens**, usually right when you add the plugin. See
[Host crashes when adding the plugin](troubleshooting.md#host-crashes-when-adding-the-plugin)
for a software OpenGL that fixes this.

## Install the plugin

Copy the plugin into your system's plugin folder:

=== "Windows"

    | Format | Folder |
    |---|---|
    | VST3 | `C:\Program Files\Common Files\VST3\` |
    | CLAP | `C:\Program Files\Common Files\CLAP\` |

    `WHIP Send.vst3` is a folder; copy the whole folder.

=== "macOS"

    | Format | Folder |
    |---|---|
    | VST3 | `~/Library/Audio/Plug-Ins/VST3/` |
    | CLAP | `~/Library/Audio/Plug-Ins/CLAP/` |

    The bundles are not notarized. After copying, remove the quarantine flag,
    or macOS will refuse to load them:

    ```sh
    xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/"WHIP Send.vst3"
    xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/CLAP/"WHIP Send.clap"
    ```

=== "Linux"

    | Format | Folder |
    |---|---|
    | VST3 | `~/.vst3/` |
    | CLAP | `~/.clap/` |

## Make Reaper find it

1. Open *Options → Preferences → Plug-ins → VST* (CLAP plugins are under
   *Plug-ins → CLAP*).
2. Make sure the folder is in the plugin path.
3. Click **Re-scan**.

The plugin appears as **WHIP Send** by *stblassitude*, in the *Tools* and
*Network* categories.

!!! note "Testing status"
    The plugin passes Steinberg's VST3 validator. It has been tested end to
    end on macOS against Galène 1.2. On Windows, it loads and its editor
    opens in Reaper 7.8; streaming still needs testing there. Linux builds
    pass CI but still need testing in Reaper.
