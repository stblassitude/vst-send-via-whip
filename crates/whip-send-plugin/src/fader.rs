use nih_plug::prelude::*;
use nih_plug_egui::egui::{
    self, pos2, vec2, Align2, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget,
};

/// Scale marks under the track, in dB.
const MARKS_DB: [f32; 7] = [-30.0, -20.0, -10.0, -6.0, 0.0, 6.0, 12.0];
/// How much slower the cap moves while Shift is held.
const FINE: f32 = 0.1;
const WIDTH: f32 = 240.0;
const CAP: egui::Vec2 = vec2(12.0, 20.0);
/// Room at both ends for the cap and the outermost scale labels.
const PAD: f32 = 12.0;

/// A mixing-desk style fader for a gain parameter: a groove with a cap that
/// is dragged relative to where it was grabbed, and a dB scale underneath.
/// Shift-drag for fine control, double-click or Cmd/Ctrl-click to reset.
pub(crate) struct Fader<'a> {
    param: &'a FloatParam,
    setter: &'a ParamSetter<'a>,
}

impl<'a> Fader<'a> {
    pub(crate) fn new(param: &'a FloatParam, setter: &'a ParamSetter<'a>) -> Self {
        Self { param, setter }
    }

    /// A label for the fader's grid row, level with the groove. A plain
    /// label would be centered on the row, which includes the scale.
    pub(crate) fn label(ui: &mut Ui, text: &str) {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            let galley = ui.painter().layout_no_wrap(
                text.to_string(),
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().text_color(),
            );
            let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x, CAP.y), Sense::hover());
            let pos = rect.left_center() - vec2(0.0, galley.size().y / 2.0);
            ui.painter().galley(pos, galley, ui.visuals().text_color());
        });
    }

    fn reset(&self) {
        self.setter.begin_set_parameter(self.param);
        self.setter
            .set_parameter(self.param, self.param.default_plain_value());
        self.setter.end_set_parameter(self.param);
    }
}

impl Widget for Fader<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        // Top-aligned, so the readout lines up with the groove rather than
        // with the middle of groove and scale.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            let response = self.track(ui);
            let (rect, _) = ui.allocate_exact_size(vec2(56.0, CAP.y), Sense::hover());
            ui.painter().text(
                rect.left_center(),
                Align2::LEFT_CENTER,
                self.param.to_string(),
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().text_color(),
            );
            response
        })
        .inner
    }
}

impl Fader<'_> {
    fn track(&self, ui: &mut Ui) -> Response {
        // The scale labels, plus some room before the next row.
        let label_height = 20.0;
        let (rect, mut response) =
            ui.allocate_exact_size(vec2(WIDTH, CAP.y + label_height), Sense::click_and_drag());
        let x_range = (rect.left() + PAD)..=(rect.right() - PAD);
        let travel = rect.width() - 2.0 * PAD;
        let x_of = |norm: f32| egui::emath::lerp(x_range.clone(), norm);
        let groove_y = rect.top() + CAP.y / 2.0;

        let memory = response.id.with("drag");
        if response.drag_started() {
            self.setter.begin_set_parameter(self.param);
            let mut start = self.param.unmodulated_normalized_value();
            // Grabbing the track away from the cap moves the cap there first.
            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                if (p.x - x_of(start)).abs() > CAP.x / 2.0 {
                    start = egui::emath::remap_clamp(p.x, x_range.clone(), 0.0..=1.0);
                    self.setter
                        .set_parameter(self.param, self.param.preview_plain(start));
                    response.mark_changed();
                }
            }
            ui.memory_mut(|m| m.data.insert_temp(memory, (start, 0.0f32)));
        }
        if response.dragged() {
            let (start, moved) = ui
                .memory(|m| m.data.get_temp::<(f32, f32)>(memory))
                .unwrap_or((self.param.unmodulated_normalized_value(), 0.0));
            let scale = if ui.input(|i| i.modifiers.shift) {
                FINE
            } else {
                1.0
            };
            let moved = moved + response.drag_delta().x * scale;
            ui.memory_mut(|m| m.data.insert_temp(memory, (start, moved)));
            let norm = (start + moved / travel).clamp(0.0, 1.0);
            let value = self.param.preview_plain(norm);
            if value != self.param.unmodulated_plain_value() {
                self.setter.set_parameter(self.param, value);
                response.mark_changed();
            }
        }
        if response.drag_stopped() {
            self.setter.end_set_parameter(self.param);
        }
        if response.double_clicked() || (response.clicked() && ui.input(|i| i.modifiers.command)) {
            self.reset();
            response.mark_changed();
        }

        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            let visuals = ui.visuals();
            let weak = visuals.weak_text_color();

            let groove = Rect::from_min_max(
                pos2(*x_range.start(), groove_y - 2.0),
                pos2(*x_range.end(), groove_y + 2.0),
            );
            painter.rect(
                groove,
                2.0,
                visuals.extreme_bg_color,
                visuals.widgets.noninteractive.bg_stroke,
                StrokeKind::Inside,
            );

            let tick_top = rect.top() + CAP.y - 2.0;
            for db in MARKS_DB {
                let x = x_of(self.param.preview_normalized(util::db_to_gain(db)));
                let stroke = Stroke::new(if db == 0.0 { 1.5_f32 } else { 1.0 }, weak);
                painter.line_segment([pos2(x, tick_top), pos2(x, tick_top + 4.0)], stroke);
                let text = if db > 0.0 {
                    format!("+{db}")
                } else {
                    format!("{db}")
                };
                painter.text(
                    pos2(x, tick_top + 4.0),
                    Align2::CENTER_TOP,
                    text,
                    FontId::proportional(10.0),
                    weak,
                );
            }

            let style = ui.style().interact(&response);
            let x = x_of(self.param.unmodulated_normalized_value());
            let cap = Rect::from_center_size(pos2(x, groove_y), CAP);
            painter.rect(cap, 2.0, style.bg_fill, style.fg_stroke, StrokeKind::Inside);
            painter.line_segment(
                [pos2(x, cap.top() + 3.0), pos2(x, cap.bottom() - 3.0)],
                Stroke::new(2.0_f32, visuals.selection.bg_fill),
            );
        }

        response.on_hover_text("Drag to set, Shift for fine control, double-click for 0 dB")
    }
}
