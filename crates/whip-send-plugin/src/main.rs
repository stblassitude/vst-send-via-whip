//! Standalone build for testing without a DAW. See `--help` for the audio
//! device options. The stream can be preconfigured from the environment:
//!
//! - `WHIP_ENDPOINT`, `WHIP_TOKEN`: initial endpoint and token
//! - `WHIP_HEADLESS=1`: run without the editor window (stop with Ctrl-C)

use nih_plug::prelude::*;
use whip_core::StreamSettings;
use whip_send_plugin::{standalone, WhipSend};

fn main() {
    let settings = StreamSettings {
        endpoint: std::env::var("WHIP_ENDPOINT").unwrap_or_default(),
        token: std::env::var("WHIP_TOKEN").unwrap_or_default(),
        ..Default::default()
    };
    let headless = std::env::var("WHIP_HEADLESS").is_ok_and(|v| v == "1");
    standalone::configure(settings, headless);
    nih_export_standalone::<WhipSend>();
}
