//! WHIP Send: publishes a track's audio to a WHIP endpoint (e.g. Galene)
//! while passing it through unchanged.

use std::num::NonZeroU32;
use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use rtrb::Consumer;
use whip_core::{
    audio_channel, ChannelMode, Feeder, Sender, SenderOptions, Shared, StreamSettings,
};

mod editor;
pub mod standalone;

pub struct WhipSend {
    params: Arc<WhipSendParams>,
    shared: Arc<Shared>,
    feeder: Feeder,
    consumer: Option<Consumer<f32>>,
    sender: Option<Sender>,
    offline: bool,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    #[name = "Mono (left)"]
    MonoLeft,
    #[name = "Mono (right)"]
    MonoRight,
    #[name = "Mono (L+R)"]
    MonoSum,
    #[name = "Stereo"]
    Stereo,
}

impl From<Mode> for ChannelMode {
    fn from(m: Mode) -> Self {
        match m {
            Mode::MonoLeft => ChannelMode::MonoLeft,
            Mode::MonoRight => ChannelMode::MonoRight,
            Mode::MonoSum => ChannelMode::MonoSum,
            Mode::Stereo => ChannelMode::Stereo,
        }
    }
}

#[derive(Params)]
pub struct WhipSendParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    /// Endpoint, token and encoder settings. Stored in the project.
    #[persist = "stream"]
    pub stream: Arc<RwLock<StreamSettings>>,

    #[id = "enabled"]
    pub enabled: BoolParam,

    #[id = "gain"]
    pub gain: FloatParam,

    #[id = "mode"]
    pub mode: EnumParam<Mode>,
}

impl WhipSendParams {
    fn new(shared: &Arc<Shared>) -> Self {
        let s = shared.clone();
        let enabled =
            BoolParam::new("Send enabled", true).with_callback(Arc::new(move |v| s.set_enabled(v)));
        let s = shared.clone();
        let gain = FloatParam::new(
            "Send gain",
            util::db_to_gain(0.0),
            FloatRange::Skewed {
                min: util::db_to_gain(-30.0),
                max: util::db_to_gain(12.0),
                factor: FloatRange::gain_skew_factor(-30.0, 12.0),
            },
        )
        .with_smoother(SmoothingStyle::None)
        .with_unit(" dB")
        .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
        .with_string_to_value(formatters::s2v_f32_gain_to_db())
        .with_callback(Arc::new(move |v| s.set_gain(v)));
        let s = shared.clone();
        let mode = EnumParam::new("Channels", Mode::MonoLeft)
            .with_callback(Arc::new(move |v: Mode| s.set_channel_mode(v.into())));
        Self {
            editor_state: EguiState::from_size(460, 560),
            stream: Arc::new(RwLock::new(standalone::initial_settings())),
            enabled,
            gain,
            mode,
        }
    }
}

impl Default for WhipSend {
    fn default() -> Self {
        let shared = Shared::new();
        let (feeder, consumer) = audio_channel(shared.clone());
        Self {
            params: Arc::new(WhipSendParams::new(&shared)),
            shared,
            feeder,
            consumer: Some(consumer),
            sender: None,
            offline: false,
        }
    }
}

impl WhipSend {
    /// Copies parameter values into the shared atomics. Callbacks cover
    /// automation and the editor; this covers state restored on load.
    fn sync_params(&self) {
        self.shared.set_enabled(self.params.enabled.value());
        self.shared.set_gain(self.params.gain.value());
        self.shared
            .set_channel_mode(self.params.mode.value().into());
    }
}

impl Plugin for WhipSend {
    const NAME: &'static str = "WHIP Send";
    const VENDOR: &'static str = "stblassitude";
    const URL: &'static str = "https://github.com/stblassitude/vst-send-via-whip";
    const EMAIL: &'static str = "stb@lassitu.de";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        if standalone::headless() {
            return None;
        }
        editor::create(self.params.clone(), self.shared.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.shared
            .set_sample_rate(buffer_config.sample_rate.round() as u32);
        self.offline = buffer_config.process_mode == ProcessMode::Offline;
        self.sync_params();
        self.feeder.reset_timing();
        // Started here rather than in Default, so hosts that only scan the
        // plugin never start a network thread.
        if self.sender.is_none() {
            if let Some(consumer) = self.consumer.take() {
                match Sender::spawn(
                    self.params.stream.clone(),
                    self.shared.clone(),
                    consumer,
                    SenderOptions::default(),
                ) {
                    Ok(sender) => self.sender = Some(sender),
                    Err(e) => nih_error!("cannot start the sender thread: {e}"),
                }
            }
        }
        true
    }

    fn reset(&mut self) {
        self.feeder.reset_timing();
    }

    fn deactivate(&mut self) {
        self.feeder.reset_timing();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Offline renders run faster than real time; don't stream them.
        if self.offline {
            return ProcessStatus::Normal;
        }
        let channels = buffer.as_slice_immutable();
        match channels {
            [] => {}
            [mono] => self.feeder.push(mono, mono),
            [left, right, ..] => self.feeder.push(left, right),
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for WhipSend {
    const CLAP_ID: &'static str = "de.lassitu.whip-send";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Publishes the track's audio to a WHIP endpoint such as Galene");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Utility,
        ClapFeature::Stereo,
        ClapFeature::Mono,
    ];
}

impl Vst3Plugin for WhipSend {
    const VST3_CLASS_ID: [u8; 16] = *b"WhipSendStbVoc01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Tools,
        Vst3SubCategory::Network,
    ];
}

nih_export_clap!(WhipSend);
nih_export_vst3!(WhipSend);
