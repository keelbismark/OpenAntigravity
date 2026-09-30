//! The small set of controls every screen is built from.
//!
//! There are deliberately only four: a switch, a card, a primary button and a
//! link. The brief was that everything is driven by switches — so a control
//! that is not one of these is a design mistake, not a missing widget.

use eframe::egui::{self, CornerRadius, Sense, Stroke};

use super::theme;

/// A clean, tactile toggle switch with high-contrast active state and smooth animation.
pub fn switch(ui: &mut egui::Ui, on: &mut bool, enabled: bool) -> egui::Response {
    let size = egui::vec2(38.0, 20.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut resp) = ui.allocate_exact_size(size, sense);

    if enabled && resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let how = ui.ctx().animate_bool_with_time(resp.id, *on, 0.15);
        let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered() && enabled, 0.10);

        let track_off = theme::SUNKEN;
        let track_on = if enabled {
            theme::OK
        } else {
            theme::OK.gamma_multiply(0.4)
        };
        let track = egui::Color32::from_rgb(
            egui::lerp((track_off.r() as f32)..=(track_on.r() as f32), how) as u8,
            egui::lerp((track_off.g() as f32)..=(track_on.g() as f32), how) as u8,
            egui::lerp((track_off.b() as f32)..=(track_on.b() as f32), how) as u8,
        );

        let knob_off = theme::MUTED;
        let knob_on = if enabled {
            egui::Color32::WHITE
        } else {
            theme::MUTED.gamma_multiply(0.7)
        };
        let knob = egui::Color32::from_rgb(
            egui::lerp((knob_off.r() as f32)..=(knob_on.r() as f32), how) as u8,
            egui::lerp((knob_off.g() as f32)..=(knob_on.g() as f32), how) as u8,
            egui::lerp((knob_off.b() as f32)..=(knob_on.b() as f32), how) as u8,
        );

        let stroke_off = if hover > 0.01 {
            theme::LINE.gamma_multiply(1.0 + 0.3 * hover)
        } else {
            theme::LINE
        };
        let stroke_on = if hover > 0.01 {
            theme::OK.gamma_multiply(1.1)
        } else {
            theme::OK
        };
        let stroke_color = egui::Color32::from_rgb(
            egui::lerp((stroke_off.r() as f32)..=(stroke_on.r() as f32), how) as u8,
            egui::lerp((stroke_off.g() as f32)..=(stroke_on.g() as f32), how) as u8,
            egui::lerp((stroke_off.b() as f32)..=(stroke_on.b() as f32), how) as u8,
        );

        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(10), track);
        painter.rect_stroke(
            rect,
            CornerRadius::same(10),
            Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );

        let r = rect.height() / 2.0 - 2.5;
        let cx = egui::lerp((rect.left() + r + 2.5)..=(rect.right() - r - 2.5), how);
        let center = egui::pos2(cx, rect.center().y);

        // Subtle drop shadow under active knob for depth
        if how > 0.1 {
            painter.circle_filled(
                egui::pos2(center.x, center.y + 0.8),
                r,
                egui::Color32::from_black_alpha((35.0 * how) as u8),
            );
        }
        painter.circle_filled(center, r, knob);
    }

    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A crisp status indicator dot.
pub fn pulsing_dot(ui: &mut egui::Ui, color: egui::Color32, active: bool) {
    let size = egui::vec2(12.0, 12.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let center = rect.center();
        if active {
            painter.circle_filled(center, 5.0, color.gamma_multiply(0.25));
            painter.circle_filled(center, 3.5, color);
        } else {
            painter.circle_filled(center, 3.0, color.gamma_multiply(0.4));
        }
    }
}

/// Clean section header in subtle uppercase.
pub fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(11.5)
            .strong()
            .color(theme::SUBTLE),
    );
}

/// Unified vector icons — renders consistently across OSes without emojis or tofu squares.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Icon {
    Refresh { spin: bool },
    Export,
    Folder,
    Desktop,
    Copy,
    Trash,
    Update,
    Plus,
    Cross,
    Bolt,
    Gateway,
    Check,
}

/// Paints a crisp vector icon inside a target rect.
pub fn paint_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: Icon,
    color: egui::Color32,
    time: f64,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let left = rect.left();
    let right = rect.right();
    let top = rect.top();
    let bottom = rect.bottom();
    let stroke = Stroke::new(1.2, color);

    match icon {
        Icon::Refresh { spin } => {
            let center = rect.center();
            let radius = (rect.width().min(rect.height()) / 2.0 - 1.2).max(2.5);
            let rot = if spin { (time * 8.0) as f32 } else { 0.0 };
            let sweep = std::f32::consts::PI * 1.65;
            let n = 12;
            let mut pts = Vec::with_capacity(n);
            for i in 0..n {
                let frac = i as f32 / (n - 1) as f32;
                let a = rot + frac * sweep;
                pts.push(egui::pos2(center.x + radius * a.cos(), center.y + radius * a.sin()));
            }
            for w in pts.windows(2) {
                painter.line_segment([w[0], w[1]], stroke);
            }
            if let Some(&tip) = pts.last() {
                let end_a = rot + sweep;
                let tang = end_a + std::f32::consts::FRAC_PI_2;
                let alen = 3.0;
                let p1 = egui::pos2(tip.x - alen * (tang - 0.55).cos(), tip.y - alen * (tang - 0.55).sin());
                let p2 = egui::pos2(tip.x - alen * (tang + 0.55).cos(), tip.y - alen * (tang + 0.55).sin());
                painter.line_segment([p1, tip], stroke);
                painter.line_segment([p2, tip], stroke);
            }
        }
        Icon::Export => {
            painter.line_segment([egui::pos2(cx, top + 1.0), egui::pos2(cx, cy + 2.0)], stroke);
            painter.line_segment([egui::pos2(cx - 3.0, cy - 1.0), egui::pos2(cx, cy + 2.0)], stroke);
            painter.line_segment([egui::pos2(cx + 3.0, cy - 1.0), egui::pos2(cx, cy + 2.0)], stroke);

            painter.line_segment([egui::pos2(left + 1.5, cy + 1.5), egui::pos2(left + 1.5, bottom - 1.0)], stroke);
            painter.line_segment([egui::pos2(left + 1.5, bottom - 1.0), egui::pos2(right - 1.5, bottom - 1.0)], stroke);
            painter.line_segment([egui::pos2(right - 1.5, bottom - 1.0), egui::pos2(right - 1.5, cy + 1.5)], stroke);
        }
        Icon::Folder => {
            let f_top = top + 2.5;
            let f_bottom = bottom - 1.5;
            let f_left = left + 1.0;
            let f_right = right - 1.0;
            painter.line_segment([egui::pos2(f_left, f_top), egui::pos2(f_left + 3.5, f_top)], stroke);
            painter.line_segment([egui::pos2(f_left + 3.5, f_top), egui::pos2(f_left + 5.0, f_top + 1.5)], stroke);
            painter.line_segment([egui::pos2(f_left + 5.0, f_top + 1.5), egui::pos2(f_right, f_top + 1.5)], stroke);

            painter.line_segment([egui::pos2(f_left, f_top), egui::pos2(f_left, f_bottom)], stroke);
            painter.line_segment([egui::pos2(f_left, f_bottom), egui::pos2(f_right, f_bottom)], stroke);
            painter.line_segment([egui::pos2(f_right, f_bottom), egui::pos2(f_right, f_top + 1.5)], stroke);
        }
        Icon::Desktop => {
            let m_bottom = bottom - 4.0;
            painter.rect_stroke(
                egui::Rect::from_min_max(egui::pos2(left + 1.0, top + 1.5), egui::pos2(right - 1.0, m_bottom)),
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment([egui::pos2(cx, m_bottom), egui::pos2(cx, bottom - 1.0)], stroke);
            painter.line_segment([egui::pos2(cx - 3.5, bottom - 1.0), egui::pos2(cx + 3.5, bottom - 1.0)], stroke);
        }
        Icon::Copy => {
            painter.line_segment([egui::pos2(left + 3.5, top + 1.0), egui::pos2(right - 1.0, top + 1.0)], stroke);
            painter.line_segment([egui::pos2(right - 1.0, top + 1.0), egui::pos2(right - 1.0, bottom - 3.5)], stroke);

            painter.rect_stroke(
                egui::Rect::from_min_max(egui::pos2(left + 1.0, top + 3.5), egui::pos2(right - 3.5, bottom - 1.0)),
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Trash => {
            painter.line_segment([egui::pos2(left + 2.5, top + 3.5), egui::pos2(left + 3.5, bottom - 1.0)], stroke);
            painter.line_segment([egui::pos2(left + 3.5, bottom - 1.0), egui::pos2(right - 3.5, bottom - 1.0)], stroke);
            painter.line_segment([egui::pos2(right - 3.5, bottom - 1.0), egui::pos2(right - 2.5, top + 3.5)], stroke);

            painter.line_segment([egui::pos2(left + 1.0, top + 3.5), egui::pos2(right - 1.0, top + 3.5)], stroke);
            painter.line_segment([egui::pos2(cx - 2.0, top + 3.5), egui::pos2(cx - 2.0, top + 1.5)], stroke);
            painter.line_segment([egui::pos2(cx - 2.0, top + 1.5), egui::pos2(cx + 2.0, top + 1.5)], stroke);
            painter.line_segment([egui::pos2(cx + 2.0, top + 1.5), egui::pos2(cx + 2.0, top + 3.5)], stroke);
        }
        Icon::Update => {
            painter.line_segment([egui::pos2(cx, bottom - 3.0), egui::pos2(cx, top + 1.0)], stroke);
            painter.line_segment([egui::pos2(cx - 3.0, top + 4.0), egui::pos2(cx, top + 1.0)], stroke);
            painter.line_segment([egui::pos2(cx + 3.0, top + 4.0), egui::pos2(cx, top + 1.0)], stroke);
            painter.line_segment([egui::pos2(left + 2.0, bottom - 1.0), egui::pos2(right - 2.0, bottom - 1.0)], stroke);
        }
        Icon::Plus => {
            painter.line_segment([egui::pos2(cx - 3.5, cy), egui::pos2(cx + 3.5, cy)], stroke);
            painter.line_segment([egui::pos2(cx, cy - 3.5), egui::pos2(cx, cy + 3.5)], stroke);
        }
        Icon::Cross => {
            painter.line_segment([egui::pos2(cx - 3.0, cy - 3.0), egui::pos2(cx + 3.0, cy + 3.0)], stroke);
            painter.line_segment([egui::pos2(cx - 3.0, cy + 3.0), egui::pos2(cx + 3.0, cy - 3.0)], stroke);
        }
        Icon::Bolt => {
            let pts = [
                egui::pos2(cx + 0.5, top + 1.0),
                egui::pos2(cx - 2.5, cy + 0.5),
                egui::pos2(cx - 0.5, cy + 0.5),
                egui::pos2(cx - 1.5, bottom - 1.0),
                egui::pos2(cx + 2.5, cy - 0.5),
                egui::pos2(cx + 0.5, cy - 0.5),
            ];
            for i in 0..pts.len() {
                painter.line_segment([pts[i], pts[(i + 1) % pts.len()]], stroke);
            }
        }
        Icon::Gateway => {
            let r1 = egui::Rect::from_min_max(egui::pos2(left + 1.0, cy - 4.5), egui::pos2(right - 1.0, cy - 1.0));
            let r2 = egui::Rect::from_min_max(egui::pos2(left + 1.0, cy + 1.0), egui::pos2(right - 1.0, cy + 4.5));
            painter.rect_stroke(r1, CornerRadius::same(1), stroke, egui::StrokeKind::Inside);
            painter.rect_stroke(r2, CornerRadius::same(1), stroke, egui::StrokeKind::Inside);
            painter.circle_filled(egui::pos2(left + 3.0, cy - 2.7), 1.0, color);
            painter.circle_filled(egui::pos2(left + 3.0, cy + 2.7), 1.0, color);
        }
        Icon::Check => {
            painter.line_segment([egui::pos2(left + 2.0, cy + 0.5), egui::pos2(cx - 0.5, bottom - 2.0)], stroke);
            painter.line_segment([egui::pos2(cx - 0.5, bottom - 2.0), egui::pos2(right - 1.5, top + 2.0)], stroke);
        }
    }
}

/// A clean button with optional vector icon and hover state.
pub fn btn(
    ui: &mut egui::Ui,
    icon: Option<Icon>,
    text: &str,
    text_size: f32,
    min_size: Option<egui::Vec2>,
) -> egui::Response {
    let font_id = egui::FontId::proportional(text_size);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font_id, theme::TEXT);
    let icon_w = if icon.is_some() { 13.0 + 6.0 } else { 0.0 };
    let pad_x = 10.0;
    let pad_y = 5.0;
    let mut desired = egui::vec2(
        galley.size().x + icon_w + pad_x * 2.0,
        galley.size().y + pad_y * 2.0,
    );
    if let Some(m) = min_size {
        desired.x = desired.x.max(m.x);
        desired.y = desired.y.max(m.y);
    }

    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if resp.hovered() {
            theme::ACCENT_SUBTLE
        } else {
            theme::SUNKEN
        };
        let stroke_color = if resp.hovered() {
            theme::LINE.gamma_multiply(1.4)
        } else {
            theme::LINE
        };
        painter.rect(
            rect,
            CornerRadius::same(theme::RADIUS_SMALL),
            fill,
            Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );

        let content_width = galley.size().x + icon_w;
        let start_x = rect.center().x - content_width / 2.0;
        let icon_col = if resp.hovered() { egui::Color32::WHITE } else { theme::MUTED };
        let text_col = if resp.hovered() { egui::Color32::WHITE } else { theme::TEXT };

        if let Some(ic) = icon {
            if let Icon::Refresh { spin: true } = ic {
                ui.ctx().request_repaint();
            }
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(start_x + 6.5, rect.center().y),
                egui::vec2(13.0, 13.0),
            );
            paint_icon(painter, icon_rect, ic, icon_col, ui.input(|i| i.time));
        }

        let text_pos = egui::pos2(start_x + icon_w, rect.center().y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, text_col);
    }

    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A compact status/metric chip (e.g. Bolt with "38 мс", Gateway with "127.0.0.1:45318").
pub fn metric_chip(ui: &mut egui::Ui, icon: Option<Icon>, text: &str) -> egui::Response {
    let font_id = egui::FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font_id, theme::MUTED);
    let icon_w = if icon.is_some() { 12.0 + 5.0 } else { 0.0 };
    let padding = egui::vec2(8.0, 3.5);
    let desired_size = egui::vec2(
        galley.size().x + icon_w + padding.x * 2.0,
        galley.size().y + padding.y * 2.0,
    );
    let (rect, resp) = ui.allocate_exact_size(desired_size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if resp.hovered() { theme::ACCENT_SUBTLE } else { theme::SUNKEN };
        painter.rect(
            rect,
            CornerRadius::same(theme::RADIUS_SMALL),
            fill,
            Stroke::new(1.0, theme::LINE),
            egui::StrokeKind::Inside,
        );

        let icon_col = if resp.hovered() { theme::TEXT } else { theme::MUTED };
        let text_col = if resp.hovered() { theme::TEXT } else { theme::MUTED };

        let start_x = rect.left() + padding.x;
        if let Some(ic) = icon {
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(start_x + 6.0, rect.center().y),
                egui::vec2(12.0, 12.0),
            );
            paint_icon(painter, icon_rect, ic, icon_col, ui.input(|i| i.time));
        }

        let text_pos = egui::pos2(start_x + icon_w, rect.top() + padding.y);
        painter.galley(text_pos, galley, text_col);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Clean navigation tabs matching the website (active tab has white text and underline).
pub fn site_tabs<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        for (val, label) in options {
            let is_active = *val == *current;
            let text_color = if is_active {
                theme::TEXT
            } else {
                theme::MUTED
            };
            let font = egui::FontId::proportional(13.5);
            let galley = ui.painter().layout_no_wrap(label.to_string(), font, text_color);
            let size = egui::vec2(galley.size().x, 28.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());

            if resp.clicked() && !is_active {
                *current = *val;
                changed = true;
            }

            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                let text_pos = egui::pos2(rect.left(), rect.top() + 4.0);
                let col = if is_active {
                    theme::TEXT
                } else if resp.hovered() {
                    egui::Color32::WHITE
                } else {
                    theme::MUTED
                };
                painter.galley(text_pos, galley, col);

                if is_active {
                    let line_y = rect.bottom() - 1.0;
                    painter.line_segment(
                        [egui::pos2(rect.left(), line_y), egui::pos2(rect.right(), line_y)],
                        Stroke::new(2.0, egui::Color32::WHITE),
                    );
                }
            }
        }
    });

    // Subtle baseline underneath tab navigation
    let avail_w = ui.available_width();
    let (line_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, 1.0), Sense::hover());
    ui.painter().line_segment(
        [line_rect.left_top(), line_rect.right_top()],
        Stroke::new(1.0, theme::LINE),
    );

    changed
}

/// A styled modern pill badge matching the website (.pill-badge).
pub fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    let font_id = egui::FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font_id, theme::TEXT);
    let padding = egui::vec2(8.0, 3.5);
    let desired_size = egui::vec2(galley.size().x + padding.x * 2.0 + 10.0, galley.size().y + padding.y * 2.0);
    let (rect, _) = ui.allocate_exact_size(desired_size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let bg = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12);
        let stroke = Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 30));
        painter.rect(
            rect,
            CornerRadius::same(theme::RADIUS_PILL),
            bg,
            stroke,
            egui::StrokeKind::Inside,
        );
        let dot_center = egui::pos2(rect.left() + padding.x + 1.0, rect.center().y);
        painter.circle_filled(dot_center, 3.0, color);
        let text_pos = egui::pos2(rect.left() + padding.x + 9.0, rect.top() + padding.y);
        painter.galley(text_pos, galley, theme::TEXT);
    }
}

/// Room reserved on the right of a row for its switch: the 42 px control plus
/// the gap that keeps it off the card's edge.
pub const SWITCH_COLUMN: f32 = 62.0;

/// A row of text with a switch pinned to its right.
pub fn switch_row(
    ui: &mut egui::Ui,
    on: &mut bool,
    enabled: bool,
    text: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let text_w = (ui.available_width() - SWITCH_COLUMN).max(120.0);
        ui.allocate_ui_with_layout(
            egui::vec2(text_w, 0.0),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.set_max_width(text_w);
                text(ui);
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = switch(ui, on, enabled).changed();
        });
    });
    changed
}

/// A titled block. Everything on the main screen lives in one of these.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(14))
        .stroke(Stroke::new(1.0, theme::CARD_BORDER))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Renders a modern latency monitor / sparkline chart.
pub fn latency_chart(
    ui: &mut egui::Ui,
    samples: &[u32],
    active_rtt: Option<u32>,
    label: &str,
) {
    let height = 48.0;
    let width = ui.available_width().max(160.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        // Background well with subtle border
        painter.rect(
            rect,
            CornerRadius::same(theme::RADIUS_SMALL),
            theme::SUNKEN,
            Stroke::new(1.0, theme::LINE),
            egui::StrokeKind::Inside,
        );

        if samples.is_empty() {
            let msg = if let Some(rtt) = active_rtt {
                format!("{label}: {rtt} мс")
            } else {
                format!("{label}: ожидание данных…")
            };
            let font = egui::FontId::proportional(12.0);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                msg,
                font,
                theme::MUTED,
            );
            return;
        }

        let max_val = (*samples.iter().max().unwrap_or(&100)).max(80) as f32;
        let min_val = (*samples.iter().min().unwrap_or(&0)) as f32;
        let avg_val = (samples.iter().sum::<u32>() as f32 / samples.len() as f32).round() as u32;

        let padding_x = 8.0;
        let padding_y = 6.0;
        let graph_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + padding_x, rect.top() + 18.0),
            egui::pos2(rect.right() - padding_x, rect.bottom() - padding_y),
        );

        let n = samples.len();
        let step_x = if n > 1 {
            graph_rect.width() / (n - 1) as f32
        } else {
            graph_rect.width()
        };

        // Compute points
        let points: Vec<egui::Pos2> = samples
            .iter()
            .enumerate()
            .map(|(i, &val)| {
                let x = graph_rect.left() + i as f32 * step_x;
                let norm = (val as f32 / max_val).clamp(0.05, 0.95);
                let y = graph_rect.bottom() - norm * graph_rect.height();
                egui::pos2(x, y)
            })
            .collect();

        // Line color based on current latency
        let current = samples.last().copied().or(active_rtt).unwrap_or(0);
        let color = if current < 120 {
            theme::OK
        } else if current < 300 {
            theme::WARN
        } else {
            theme::BAD
        };

        // Draw soft gradient fill below line
        if points.len() >= 2 {
            let mut mesh = egui::Mesh::default();
            let fill_color_top = color.gamma_multiply(0.25);
            let fill_color_bot = color.gamma_multiply(0.02);

            for i in 0..(points.len() - 1) {
                let p1 = points[i];
                let p2 = points[i + 1];
                let b1 = egui::pos2(p1.x, graph_rect.bottom());
                let b2 = egui::pos2(p2.x, graph_rect.bottom());

                let idx = mesh.vertices.len() as u32;
                mesh.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: fill_color_top });
                mesh.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: fill_color_top });
                mesh.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: fill_color_bot });
                mesh.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: fill_color_bot });

                mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
            }
            painter.add(egui::Shape::mesh(mesh));

            // Draw line
            for i in 0..(points.len() - 1) {
                painter.line_segment([points[i], points[i + 1]], Stroke::new(2.0, color));
            }
        }

        // Pulse dot at the latest point
        if let Some(&last_pt) = points.last() {
            painter.circle_filled(last_pt, 4.0, color);
            painter.circle_filled(last_pt, 2.0, egui::Color32::WHITE);
        }

        // Overlay text: current RTT & stats
        let stats_text = format!("{label}: {current} мс · сред: {avg_val} мс · мин: {} · макс: {}", min_val as u32, max_val as u32);
        painter.text(
            egui::pos2(rect.left() + 8.0, rect.top() + 4.0),
            egui::Align2::LEFT_TOP,
            stats_text,
            egui::FontId::proportional(11.0),
            theme::TEXT,
        );
    }
}

/// The one filled button per screen — the action the user came to press.
/// Styled like the website CTA (.btn-primary): bold black text on crisp white background.
pub fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(text)
            .color(if enabled {
                egui::Color32::from_rgb(0x0A, 0x0A, 0x0C)
            } else {
                theme::MUTED
            })
            .size(14.0)
            .strong(),
    )
    .fill(if enabled {
        egui::Color32::WHITE
    } else {
        theme::SUNKEN
    })
    .stroke(if enabled {
        Stroke::NONE
    } else {
        Stroke::new(1.0, theme::LINE)
    })
    .corner_radius(CornerRadius::same(theme::RADIUS_SMALL))
    .min_size(egui::vec2(0.0, 36.0));

    ui.add_enabled(enabled, btn)
}

/// A quiet outlined button: everything that is not *the* action.
pub fn ghost(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(text)
            .color(theme::TEXT)
            .size(12.5),
    )
    .fill(theme::SUNKEN)
    .stroke(Stroke::new(1.0, theme::LINE))
    .corner_radius(CornerRadius::same(theme::RADIUS_SMALL))
    .min_size(egui::vec2(0.0, 28.0));
    ui.add(btn)
}

/// Small grey explanatory text — the line under a switch that says what it does.
pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(theme::MUTED).size(12.5));
}

/// A coloured status dot, for install rows and provider rows.
pub fn dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().circle_filled(rect.center(), 4.0, color);
    }
}
