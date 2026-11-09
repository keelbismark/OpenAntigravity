//! Card components for the Control Center: Hero status card, Quick proxy card.

use eframe::egui::{self, CornerRadius, Sense, Stroke};
use std::time::Duration;

use crate::gui::status::{self, Action, Tone};
use crate::gui::widgets::{self, Icon};
use crate::gui::{theme, App, Tab};
use crate::ops::{Cap, Cmd};

pub(super) fn quick_proxy_card(app: &mut App, ui: &mut egui::Ui) {
    let busy = app.is_busy();
    ui.horizontal(|ui| {
        widgets::section_header(ui, "ПРОКСИ");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::btn(ui, None, "Настроить →", 11.5, None).clicked() {
                app.current_tab = Tab::Proxy;
            }
        });
    });
    ui.add_space(4.0);

    widgets::card(ui, |ui| {
        let own_on = app.status.as_ref().map(|s| s.get(Cap::OwnProxy).is_on()).unwrap_or(true);
        let proxy_addr = app.own_proxy_input.trim();
        let profiles = app.status.as_ref().map(|s| s.proxy_profiles.clone()).unwrap_or_default();
        let selected_profile = profiles.iter().find(|p| p.address.trim() == proxy_addr);

        let display_addr = if !own_on {
            "Прокси отключён (прямое соединение)".to_string()
        } else if proxy_addr.is_empty() {
            "Встроенный прокси (по умолчанию)".to_string()
        } else if let Some(prof) = selected_profile {
            format!("{}: {}", prof.name, crate::diag::mask_proxy_line(proxy_addr))
        } else {
            crate::diag::mask_proxy_line(proxy_addr)
        };

        ui.horizontal(|ui| {
            let (dot, col) = if own_on {
                ("●", theme::OK)
            } else {
                ("○", theme::MUTED)
            };
            ui.label(egui::RichText::new(dot).color(col).size(12.0));
            ui.label(
                egui::RichText::new(display_addr)
                    .size(12.5)
                    .strong()
                    .color(theme::TEXT),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (ping_icon, ping_label) = if app.proxy_test_running {
                    (Icon::Refresh { spin: true }, "замер…".to_string())
                } else if let Some(rtt) = app.last_rtt {
                    (Icon::Bolt, format!("{rtt} мс"))
                } else {
                    (Icon::Bolt, "проверить".to_string())
                };

                let chip = widgets::metric_chip(ui, Some(ping_icon), &ping_label)
                    .on_hover_text("Нажмите для проверки задержки к Google API");
                if chip.clicked() && !app.proxy_test_running {
                    app.start_proxy_test(ui.ctx());
                }
            });
        });

        // Quick profile chips if profiles exist
        if !profiles.is_empty() {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Профили:")
                        .size(11.5)
                        .color(theme::MUTED),
                );

                let mut select_idx: Option<usize> = None;
                for (idx, prof) in profiles.iter().enumerate() {
                    let is_selected = app.own_proxy_input.trim() == prof.address.trim();
                    let prof_resp = widgets::profile_pill(ui, &prof.name, is_selected, !busy);
                    if prof_resp.on_hover_text(&prof.address).clicked() {
                        select_idx = Some(idx);
                    }
                }

                if let Some(i) = select_idx {
                    if let Some(p) = profiles.get(i) {
                        app.own_proxy_input = p.address.clone();
                    }
                    app.worker.send(Cmd::SelectProxyProfile(i));
                }
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Hero Status & Main Action Card
// ---------------------------------------------------------------------------

pub(super) fn facts(app: &App) -> Option<status::Facts> {
    let s = app.status.as_ref()?;
    Some(status::Facts::read(s, &app.gate, app.gate_at.elapsed()))
}

pub(super) fn hero_status_card(app: &mut App, ui: &mut egui::Ui) {
    let f_opt = facts(app);
    let (is_active, h) = if let Some(ref f) = f_opt {
        (f.bypass_on || f.patch_on, status::headline(f))
    } else {
        (
            false,
            status::Headline {
                tone: Tone::Wait,
                title: "Проверка системы…".into(),
                detail: "Определение доступности компонентов и шлюза".into(),
                action: None,
            },
        )
    };

    let _accent = match h.tone {
        Tone::Ok => theme::OK,
        Tone::Wait => theme::ACCENT,
        Tone::Fixing => theme::WARN,
        Tone::Action => theme::BAD,
        Tone::Off => theme::SUBTLE,
    };

    if let Some(ref f) = f_opt {
        if f.refusal.is_some() || f.answer.is_some() || f.verify.is_some() {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
    }

    let busy = app.is_busy();
    let mut pressed_action: Option<Action> = None;
    let mut flip_off = false;

    let card_border = theme::CARD_BORDER;

    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(14))
        .stroke(Stroke::new(1.0, card_border))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Top row: Emblem + Title & Detail
            ui.horizontal(|ui| {
                let active_t = ui.ctx().animate_bool_with_time(ui.id().with("hero_act"), is_active, 0.22);
                let emblem_bg = widgets::lerp_color(
                    theme::SUNKEN,
                    egui::Color32::from_rgba_unmultiplied(34, 197, 94, 25),
                    active_t,
                );
                let emblem_color = widgets::lerp_color(theme::MUTED, theme::OK, active_t);

                let emblem_size = egui::vec2(34.0, 34.0);
                let (emblem_rect, _) = ui.allocate_exact_size(emblem_size, Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(emblem_rect, CornerRadius::same(17), emblem_bg);
                painter.rect_stroke(
                    emblem_rect,
                    CornerRadius::same(17),
                    Stroke::new(1.0, emblem_color.gamma_multiply(0.4)),
                    egui::StrokeKind::Inside,
                );
                let c = emblem_rect.center();
                if is_active {
                    painter.line_segment(
                        [c + egui::vec2(-4.5, 0.5), c + egui::vec2(-1.0, 3.5)],
                        Stroke::new(2.2, emblem_color),
                    );
                    painter.line_segment(
                        [c + egui::vec2(-1.0, 3.5), c + egui::vec2(5.0, -3.5)],
                        Stroke::new(2.2, emblem_color),
                    );
                } else {
                    painter.circle_stroke(c, 4.5, Stroke::new(1.8, emblem_color));
                }

                ui.add_space(8.0);

                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(&h.title)
                            .size(15.5)
                            .strong()
                            .color(theme::TEXT),
                    );
                    ui.add_space(1.0);
                    ui.label(
                        egui::RichText::new(&h.detail)
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                });
            });

            // Middle: Metrics / Status strip (when connected or telemetry available)
            if is_active || app.last_rtt.is_some() || app.proxy_test_running {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    let (ping_icon, ping_label) = if app.proxy_test_running {
                        (Icon::Refresh { spin: true }, "замер…".to_string())
                    } else if let Some(rtt) = app.last_rtt {
                        (Icon::Bolt, format!("{rtt} мс"))
                    } else {
                        (Icon::Bolt, "проверить пинг".to_string())
                    };

                    let chip_resp = widgets::metric_chip(ui, Some(ping_icon), &ping_label)
                        .on_hover_text("Нажмите для повторной проверки задержки к Google API");
                    if chip_resp.clicked() && !app.proxy_test_running {
                        app.start_proxy_test(ui.ctx());
                    }

                    let gw = format!("Локальный шлюз: {}:{}", crate::proxy::LISTEN_IP, crate::proxy::port());
                    widgets::metric_chip(ui, Some(Icon::Gateway), &gw);
                });
            }

            ui.add_space(12.0);

            // Action Button: Solid, Full Width, Highly Visible
            let btn_width = ui.available_width();
            if let Some(action) = h.action {
                let btn = egui::Button::new(
                    egui::RichText::new(action.label())
                        .size(14.0)
                        .strong()
                        .color(egui::Color32::BLACK),
                )
                .fill(egui::Color32::WHITE)
                .corner_radius(CornerRadius::same(theme::RADIUS_SMALL))
                .min_size(egui::vec2(btn_width, 38.0));

                if ui.add_enabled(!busy, btn).clicked() {
                    pressed_action = Some(action);
                }
            } else if is_active {
                let btn = egui::Button::new(
                    egui::RichText::new("Отключить")
                        .size(13.5)
                        .color(theme::TEXT),
                )
                .fill(theme::SUNKEN)
                .stroke(Stroke::new(1.0, theme::LINE))
                .corner_radius(CornerRadius::same(theme::RADIUS_SMALL))
                .min_size(egui::vec2(btn_width, 36.0));

                if ui.add_enabled(!busy, btn).clicked() {
                    flip_off = true;
                }
            } else {
                let btn = egui::Button::new(
                    egui::RichText::new("Активировать")
                        .size(14.0)
                        .strong()
                        .color(egui::Color32::BLACK),
                )
                .fill(egui::Color32::WHITE)
                .corner_radius(CornerRadius::same(theme::RADIUS_SMALL))
                .min_size(egui::vec2(btn_width, 38.0));

                if ui.add_enabled(!busy, btn).clicked() {
                    pressed_action = Some(Action::EnableAll);
                }
            }
        });

    if flip_off {
        app.worker.send(Cmd::DisableAll);
    } else if let Some(action) = pressed_action {
        match action {
            Action::EnableAll => app.worker.send(Cmd::EnableAll),
            Action::Repair => app.worker.send(Cmd::Repair),
            Action::KillHolder => app.worker.send(Cmd::KillHolder),
            Action::Elevate => app.request_elevation(),
            Action::Verify => {
                if let Some(f) = f_opt {
                    if let Some((_, url)) = &f.verify {
                        crate::utils::open_url_as_user(url);
                    }
                }
            }
        }
    }
}

