//! Configuration hooks for the standalone test binary. Plugin builds never
//! call [`configure`], so these have no effect there.

use std::sync::OnceLock;

use whip_core::StreamSettings;

struct Config {
    settings: StreamSettings,
    headless: bool,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Sets the initial stream settings and whether to run without an editor.
/// Must be called before the plugin is instantiated.
pub fn configure(settings: StreamSettings, headless: bool) {
    let _ = CONFIG.set(Config { settings, headless });
}

pub(crate) fn initial_settings() -> StreamSettings {
    CONFIG.get().map(|c| c.settings.clone()).unwrap_or_default()
}

pub(crate) fn headless() -> bool {
    CONFIG.get().is_some_and(|c| c.headless)
}
