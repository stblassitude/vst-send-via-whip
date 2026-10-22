use std::sync::Arc;
use std::time::{Duration, Instant};

use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Color32, RichText};
use nih_plug_egui::widgets::ParamSlider;
use nih_plug_egui::{create_egui_editor, resizable_window::ResizableWindow};
use whip_core::{ConnState, Content, Shared, Status, StreamSettings};

use crate::{Mode, WhipSendParams};

struct EditorState {
    params: Arc<WhipSendParams>,
    shared: Arc<Shared>,
    /// What the user is editing; applied explicitly so typing doesn't
    /// reconnect on every keystroke.
    draft: StreamSettings,
    /// The settings last seen in the shared state.
    applied: StreamSettings,
    show_token: bool,
}

pub(crate) fn create(params: Arc<WhipSendParams>, shared: Arc<Shared>) -> Option<Box<dyn Editor>> {
    let current = params.stream.read().map(|s| s.clone()).unwrap_or_default();
    let state = EditorState {
        params: params.clone(),
        shared,
        draft: current.clone(),
        applied: current,
        show_token: false,
    };
    create_egui_editor(
        params.editor_state.clone(),
        state,
        |_, _| {},
        move |ctx, setter, state| {
            let egui_state = state.params.editor_state.clone();
            ResizableWindow::new("whip-send")
                .min_size(egui::Vec2::new(400.0, 480.0))
                .show(ctx, egui_state.as_ref(), |ui| {
                    draw(ui, setter, state);
                });
            ctx.request_repaint_after(Duration::from_millis(250));
        },
    )
}

fn draw(ui: &mut egui::Ui, setter: &ParamSetter, st: &mut EditorState) {
    // Pick up settings changed elsewhere, e.g. by loading a preset.
    if let Ok(current) = st.params.stream.read() {
        if *current != st.applied {
            let dirty = st.draft != st.applied;
            st.applied = current.clone();
            if !dirty {
                st.draft = current.clone();
            }
        }
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("WHIP Send");
        ui.add_space(4.0);

        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("Endpoint");
                ui.add(
                    egui::TextEdit::singleline(&mut st.draft.endpoint)
                        .hint_text("https://host/group/name/.whip")
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();

                ui.label("Token");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut st.draft.token)
                            .password(!st.show_token)
                            .desired_width(220.0),
                    );
                    ui.checkbox(&mut st.show_token, "show");
                });
                ui.end_row();

                ui.label("Bitrate");
                let mut kbps = st.draft.bitrate / 1000;
                if ui
                    .add(
                        egui::DragValue::new(&mut kbps)
                            .range(
                                StreamSettings::MIN_BITRATE / 1000
                                    ..=StreamSettings::MAX_BITRATE / 1000,
                            )
                            .suffix(" kbit/s"),
                    )
                    .changed()
                {
                    st.draft.bitrate = kbps * 1000;
                }
                ui.end_row();

                ui.label("Content");
                egui::ComboBox::from_id_salt("content")
                    .selected_text(match st.draft.content {
                        Content::Voice => "Voice",
                        Content::Music => "Music",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.draft.content, Content::Voice, "Voice");
                        ui.selectable_value(&mut st.draft.content, Content::Music, "Music");
                    });
                ui.end_row();
            });

        let dirty = st.draft != st.applied;
        ui.horizontal(|ui| {
            if ui.add_enabled(dirty, egui::Button::new("Apply")).clicked() {
                if let Ok(mut s) = st.params.stream.write() {
                    st.draft.endpoint = st.draft.endpoint.trim().to_string();
                    st.draft.token = st.draft.token.trim().to_string();
                    *s = st.draft.clone();
                    st.applied = st.draft.clone();
                }
            }
            if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                st.draft = st.applied.clone();
            }
            if ui.button("Reconnect").clicked() {
                st.shared.request_reconnect();
            }
            if dirty {
                ui.label(RichText::new("unsaved changes").italics());
            }
        });

        ui.separator();

        egui::Grid::new("params")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                let p = &st.params;
                ui.label("Send");
                let mut enabled = p.enabled.value();
                if ui.checkbox(&mut enabled, "enabled").changed() {
                    setter.begin_set_parameter(&p.enabled);
                    setter.set_parameter(&p.enabled, enabled);
                    setter.end_set_parameter(&p.enabled);
                }
                ui.end_row();

                ui.label("Gain");
                ui.add(ParamSlider::for_param(&p.gain, setter));
                ui.end_row();

                ui.label("Channels");
                let current = p.mode.value();
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(mode_name(current))
                    .show_ui(ui, |ui| {
                        for m in [Mode::MonoLeft, Mode::MonoRight, Mode::MonoSum, Mode::Stereo] {
                            if ui.selectable_label(current == m, mode_name(m)).clicked()
                                && current != m
                            {
                                setter.begin_set_parameter(&p.mode);
                                setter.set_parameter(&p.mode, m);
                                setter.end_set_parameter(&p.mode);
                            }
                        }
                    });
                ui.end_row();
            });

        ui.separator();
        draw_status(ui, &st.shared.status());
    });
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::MonoLeft => "Mono (left)",
        Mode::MonoRight => "Mono (right)",
        Mode::MonoSum => "Mono (L+R)",
        Mode::Stereo => "Stereo",
    }
}

fn draw_status(ui: &mut egui::Ui, s: &Status) {
    let (text, color) = match s.state {
        ConnState::Live => ("LIVE", Color32::from_rgb(0x2e, 0xa0, 0x43)),
        ConnState::Connecting => ("connecting", Color32::from_rgb(0xd0, 0x90, 0x10)),
        ConnState::Retrying => ("retrying", Color32::from_rgb(0xd0, 0x40, 0x30)),
        ConnState::Disabled => ("disabled", Color32::GRAY),
        ConnState::Unconfigured => ("no endpoint", Color32::GRAY),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).strong().color(color));
        if let Some(since) = s.live_since {
            ui.label(format!("for {}", format_duration(Instant::now() - since)));
        }
    });
    if !s.detail.is_empty() {
        ui.label(RichText::new(&s.detail).small());
    }

    let db = if s.peak > 0.0 {
        20.0 * s.peak.log10()
    } else {
        f32::NEG_INFINITY
    };
    let fill = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let bar_color = if db > -1.0 {
        Color32::from_rgb(0xd0, 0x40, 0x30)
    } else if db > -12.0 {
        Color32::from_rgb(0xd0, 0x90, 0x10)
    } else {
        Color32::from_rgb(0x2e, 0xa0, 0x43)
    };
    ui.add(
        egui::ProgressBar::new(fill)
            .fill(bar_color)
            .text(if db.is_finite() {
                format!("{db:.1} dBFS")
            } else {
                "silence".into()
            }),
    );

    let opt = |v: Option<f64>, unit: &str, prec: usize| match v {
        Some(v) => format!("{v:.prec$} {unit}"),
        None => "–".into(),
    };
    egui::CollapsingHeader::new("Details")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("status")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    let mut row = |k: &str, v: String| {
                        ui.label(k);
                        ui.label(v);
                        ui.end_row();
                    };
                    row(
                        "Audio",
                        format!(
                            "{} Hz, {} ch, {} frames/block",
                            s.sample_rate, s.channels, s.block_frames
                        ),
                    );
                    let i = &s.intervals;
                    row(
                        "Callbacks (last min)",
                        if i.count > 0 {
                            format!(
                                "avg {:.2} ms, max {:.2} ms, sd {:.2} ms",
                                i.mean_ms, i.max_ms, i.stddev_ms
                            )
                        } else {
                            "–".into()
                        },
                    );
                    row("Clock drift", opt(s.drift_ppm, "ppm", 1));
                    row("Queue max", format!("{:.1} ms", s.queue_max_ms));
                    row(
                        "Overruns / silence / trimmed",
                        format!(
                            "{} / {} / {} frames",
                            s.overrun_frames, s.silence_frames, s.trimmed_frames
                        ),
                    );
                    row(
                        "Sent",
                        format!(
                            "{} packets, {:.1} MB",
                            s.packets_sent,
                            s.bytes_sent as f64 / 1e6
                        ),
                    );
                    row("RTT", opt(s.rtt_ms, "ms", 1));
                    row(
                        "Loss (server)",
                        match (s.loss, s.remote_packets_lost) {
                            (Some(l), Some(n)) => format!("{:.1} %, {n} packets", l * 100.0),
                            (Some(l), None) => format!("{:.1} %", l * 100.0),
                            _ => "–".into(),
                        },
                    );
                    row("Jitter (server)", opt(s.remote_jitter_ms, "ms", 1));
                    row(
                        "NACKs / reconnects",
                        format!("{} / {}", s.nacks, s.reconnects),
                    );
                });
        });
    if let Some(path) = whip_core::log_file_path() {
        ui.label(RichText::new(format!("Log: {}", path.display())).small());
    }
}

fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
