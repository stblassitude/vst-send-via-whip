# Building

## Requirements

- **Rust:** a recent stable version (developed with 1.99), via
  [rustup](https://rustup.rs/) or a package manager.
- **CMake,** used to build libopus, which is linked statically.
- **Linux only:** the X11/OpenGL/ALSA development packages that the editor
  and standalone build need:

    ```sh
    sudo apt-get install cmake libasound2-dev libgl-dev libjack-dev libx11-xcb-dev \
      libxcb1-dev libxcb-dri2-0-dev libxcb-icccm4-dev libxcursor-dev libxkbcommon-dev \
      libxcb-shape0-dev libxcb-xfixes0-dev
    ```

On macOS with Homebrew: `brew install rust cmake`. On Windows, install the
Visual Studio Build Tools (C++ workload) and CMake.

## Build the plugin

```sh
git clone https://github.com/stblassitude/vst-send-via-whip.git
cd vst-send-via-whip
cargo xtask bundle whip-send-plugin --release
```

This produces:

- `target/bundled/WHIP Send.vst3`
- `target/bundled/WHIP Send.clap`

The bundles link only system libraries, so you can copy them to the plugin
folders as described in [Installation](../usage/installation.md).

**macOS universal bundle:** local builds are for the machine's own
architecture. For a universal bundle (as in releases):

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
MACOSX_DEPLOYMENT_TARGET=11.0 cargo xtask bundle-universal whip-send-plugin --release
```

## Other binaries

```sh
cargo build --release -p whip-send-cli                                      # test sender
cargo build --release -p whip-send-plugin --bin whip-send-standalone        # standalone plugin
```

See [Testing](testing.md) for how to use them.

## Repository layout

| Path | Contents |
|---|---|
| `crates/whip-core` | Everything except the plugin wrapper: lock-free hand-over from the audio thread, channel mode and gain, resampling, Opus encoding, the WHIP client, the WebRTC session, metrics and logging. |
| `crates/whip-send-plugin` | The nih-plug wrapper (VST3 + CLAP), the egui editor, and a standalone binary. |
| `crates/whip-send-cli` | Test sender that simulates a host. |
| `xtask` | nih-plug's bundler, run as `cargo xtask`. |
| `scripts/local-galene.sh` | Builds and runs a local Galène for testing. |
| `scripts/set-version.sh` | Sets the workspace version; used by the release workflow. |
| `docs/`, `mkdocs.yml` | This documentation. |

## Building the documentation

```sh
python3 -m venv .venv && . .venv/bin/activate
pip install -r docs/requirements.txt
mkdocs serve        # live preview at http://127.0.0.1:8000/
mkdocs build --strict
```
