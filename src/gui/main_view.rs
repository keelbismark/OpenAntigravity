//! The primary user interface: a clean, unified, native Control Center.

use eframe::egui::{self, CornerRadius, Sense, Stroke};
use std::time::Duration;

use super::status::{self, Action, Tone};
use super::{theme, widgets, App, Tab, DOCS_URL, GITHUB_RELEASES_URL, GITHUB_URL};
use crate::ops::{Cap, Cmd, Level};
use crate::utils::mask_path;

pub fn view(app: &mut App, ui: &mut egui::Ui) {
    header(app, ui);
    ui.add_space(8.0);

    let footer_h = 58.0;
    let avail_h = (ui.available_height() - footer_h).max(120.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(avail_h)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            match app.current_tab {
                Tab::Main => main_screen(app, ui),
                Tab::Logs => logs_screen(app, ui),
            }
            ui.add_space(8.0);
        });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(8.0);
    footer(app, ui);
    ui.add_space(4.0);
}

// ---------------------------------------------------------------------------
// Header & Navigation
// ---------------------------------------------------------------------------

fn header(app: &mut App, ui: &mut egui::Ui) {
    app.update_banner(ui);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Open Antigravity")
                .size(17.0)
                .strong()
                .color(theme::TEXT),
        );
        ui.add_space(4.0);

        // Version pill badge
        let ver_text = format!("v{}", crate::update::current_version());
        let font_id = egui::FontId::proportional(11.0);
        let galley = ui.painter().layout_no_wrap(ver_text, font_id, theme::MUTED);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(galley.size().x + 10.0, 18.0), Sense::hover());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            painter.rect(
                rect,
                CornerRadius::same(theme::RADIUS_SMALL),
                theme::SUNKEN,
                Stroke::new(1.0, theme::LINE),
                egui::StrokeKind::Inside,
            );
            let text_pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0);
            painter.galley(text_pos, galley, theme::MUTED);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(what) = &app.busy {
                ui.label(egui::RichText::new(what).size(12.0).color(theme::MUTED));
                ui.add(egui::Spinner::new().size(12.0));
            } else if let Some(f) = facts(app) {
                let h = status::headline(&f);
                let (accent, status_label) = match h.tone {
                    Tone::Ok => (theme::OK, "Подключено"),
                    Tone::Wait => (theme::ACCENT, "Готов к работе"),
                    Tone::Fixing => (theme::WARN, "Настройка"),
                    Tone::Action => (theme::BAD, "Внимание"),
                    Tone::Off => (theme::SUBTLE, "Отключено"),
                };
                widgets::badge(ui, status_label, accent);
            }
        });
    });

    ui.add_space(8.0);

    // Site-style underline tab navigation
    let logs_label = format!("Журнал ({})", app.log.len());
    let options = [(Tab::Main, "Обзор"), (Tab::Logs, &logs_label)];
    widgets::site_tabs(ui, &mut app.current_tab, &options);
}

// ---------------------------------------------------------------------------
// Main Control Screen
// ---------------------------------------------------------------------------

fn main_screen(app: &mut App, ui: &mut egui::Ui) {
    hero_status_card(app, ui);

    ui.add_space(14.0);
    doctor_section(app, ui);

    ui.add_space(14.0);
    proxy_section(app, ui);

    ui.add_space(14.0);
    components_section(app, ui);

    ui.add_space(14.0);
    system_section(app, ui);
}

// ---------------------------------------------------------------------------
// Hero Status & Main Action Card
// ---------------------------------------------------------------------------

fn facts(app: &App) -> Option<status::Facts> {
    let s = app.status.as_ref()?;
    Some(status::Facts::read(s, &app.gate, app.gate_at.elapsed()))
}

fn hero_status_card(app: &mut App, ui: &mut egui::Ui) {
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
                let (emblem_bg, emblem_color) = if is_active {
                    (egui::Color32::from_rgba_unmultiplied(34, 197, 94, 25), theme::OK)
                } else {
                    (theme::SUNKEN, theme::MUTED)
                };

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

                    let ping_label = if app.proxy_test_running {
                        "⚡ замер…".to_string()
                    } else if let Some(rtt) = app.last_rtt {
                        format!("⚡ {rtt} мс")
                    } else {
                        "⚡ проверить пинг".to_string()
                    };

                    let chip_resp = widgets::metric_chip(ui, "", &ping_label)
                        .on_hover_text("Нажмите для повторной проверки задержки к Google API");
                    if chip_resp.clicked() && !app.proxy_test_running {
                        app.start_proxy_test(ui.ctx());
                    }

                    let gw = format!("Локальный шлюз: {}:{}", crate::proxy::LISTEN_IP, crate::proxy::port());
                    widgets::metric_chip(ui, "🌐", &gw);
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
        app.worker.send(Cmd::Set(Cap::ClientPatch, false));
        for cap in crate::ops::bypass_order(false) {
            app.worker.send(Cmd::Set(cap, false));
        }
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

// ---------------------------------------------------------------------------
// Section: Doctor / Environment Diagnostics
// ---------------------------------------------------------------------------

fn doctor_section(app: &mut App, ui: &mut egui::Ui) {
    let busy = app.is_busy();

    ui.horizontal(|ui| {
        widgets::section_header(ui, "ДИАГНОСТИКА ОКРУЖЕНИЯ (DOCTOR)");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let running = app.diag_running || app.proxy_test_running;
            let btn_text = if running { "Тестирование…" } else { "🩺 Проверить всё" };
            let run_btn = egui::Button::new(
                egui::RichText::new(btn_text)
                    .size(11.5)
                    .color(if running { theme::MUTED } else { egui::Color32::WHITE }),
            )
            .fill(theme::SUNKEN)
            .stroke(Stroke::new(1.0, theme::LINE))
            .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

            if ui.add_enabled(!busy && !running, run_btn).clicked() {
                app.start_diagnostics(ui.ctx());
                app.start_proxy_test(ui.ctx());
            }
        });
    });
    ui.add_space(4.0);

    widgets::card(ui, |ui| {
        let status = app.status.as_ref();
        let installs = status.map(|s| s.installs.as_slice()).unwrap_or(&[]);
        let ide_found = !installs.is_empty();
        let client_patched = status.map(|s| s.client_patch.is_on()).unwrap_or(false);
        let proxy_on = status.map(|s| s.local_proxy.is_on()).unwrap_or(false);
        let port = crate::proxy::port();

        let items: [(&str, String, bool, Option<&str>); 4] = [
            (
                "Google Antigravity IDE",
                if ide_found {
                    format!("Обнаружено ({})", installs.len())
                } else {
                    "Не найдено в системе".to_string()
                },
                ide_found,
                if ide_found { None } else { Some("Установите IDE или укажите путь") },
            ),
            (
                "Патч Language Server",
                if client_patched {
                    "Активен (патч применён)".to_string()
                } else {
                    "Требуется разблокировка".to_string()
                },
                client_patched,
                if client_patched { None } else { Some("Нажмите «Активировать» выше") },
            ),
            (
                "Локальный шлюз",
                format!("127.0.0.1:{port} ({})", if proxy_on { "слушает" } else { "остановлен" }),
                proxy_on,
                None,
            ),
            (
                "Облако Google AI / Апстрим",
                if app.proxy_test_running || app.diag_running {
                    "Проверка соединения…".to_string()
                } else if let Some(rtt) = app.last_rtt {
                    format!("Связь в норме ({rtt} мс)")
                } else if let Some((res, _)) = &app.proxy_test_result {
                    match res {
                        Ok((rtt, loc)) => match loc {
                            Some(l) => format!("Доступно ({rtt} мс · {l})"),
                            None => format!("Доступно ({rtt} мс)"),
                        },
                        Err(e) => format!("Сбой: {e}"),
                    }
                } else {
                    "Готов к проверке".to_string()
                },
                app.last_rtt.is_some() || app.proxy_test_result.as_ref().map(|(r, _)| r.is_ok()).unwrap_or(false),
                None,
            ),
        ];

        for (idx, (name, val, ok, hint)) in items.iter().enumerate() {
            if idx > 0 {
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                let (dot, dot_col) = if *ok {
                    ("●", theme::OK)
                } else {
                    ("○", theme::MUTED)
                };
                ui.label(egui::RichText::new(dot).color(dot_col).size(12.0));
                ui.label(egui::RichText::new(*name).size(12.0).strong().color(theme::TEXT));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let val_col = if *ok { theme::OK } else { theme::MUTED };
                    ui.label(egui::RichText::new(val).size(11.5).color(val_col));
                });
            });
            if let Some(h) = hint {
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new(*h).size(11.0).color(theme::SUBTLE));
                });
            }
        }

        if let Some((_, diag_status, text)) = &app.diag_summary {
            ui.add_space(6.0);
            let badge_col = match diag_status {
                crate::diag::DiagStatus::Ok => theme::OK,
                crate::diag::DiagStatus::Warn => egui::Color32::from_rgb(230, 180, 50),
                crate::diag::DiagStatus::Fail => theme::BAD,
                crate::diag::DiagStatus::Info => theme::TEXT,
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Результат:").size(11.5).color(theme::SUBTLE));
                ui.label(egui::RichText::new(text).size(11.5).color(badge_col));
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Section 1: Proxy Configuration
// ---------------------------------------------------------------------------

fn proxy_section(app: &mut App, ui: &mut egui::Ui) {
    let busy = app.is_busy();
    widgets::section_header(ui, "ПРОКСИ-СЕРВЕР");
    ui.add_space(4.0);

    widgets::card(ui, |ui| {
        ui.label(
            egui::RichText::new("Адрес прокси (HTTP / SOCKS5)")
                .size(12.5)
                .strong()
                .color(theme::TEXT),
        );
        ui.add_space(4.0);

        // Full width text edit
        ui.add(
            egui::TextEdit::singleline(&mut app.own_proxy_input)
                .desired_width(f32::INFINITY)
                .hint_text("логин:пароль@хост:порт или хост:порт"),
        );

        ui.add_space(8.0);

        // Actions & Test status row
        ui.horizontal(|ui| {
            if let Some((res, _)) = &app.proxy_test_result {
                match res {
                    Ok((rtt, loc)) => {
                        let text = match loc {
                            Some(l) => format!("✓ {rtt} мс · {l}"),
                            None => format!("✓ {rtt} мс"),
                        };
                        ui.label(egui::RichText::new(text).color(theme::OK).size(12.0).strong());
                    }
                    Err(e) => {
                        ui.label(egui::RichText::new(format!("✗ {e}")).color(theme::BAD).size(11.5));
                    }
                }
            } else if app.proxy_test_running {
                ui.add(egui::Spinner::new().size(13.0));
                ui.label(egui::RichText::new("Проверка задержки…").color(theme::MUTED).size(12.0));
            } else {
                ui.label(
                    egui::RichText::new("Для резерва: proxy1;proxy2")
                        .color(theme::SUBTLE)
                        .size(11.5),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                // Save button
                let save_btn = egui::Button::new(
                    egui::RichText::new("Сохранить").size(12.0).color(theme::TEXT),
                )
                .fill(theme::SUNKEN)
                .stroke(Stroke::new(1.0, theme::LINE))
                .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

                if ui.add_enabled(!busy, save_btn).clicked() {
                    let text = app.own_proxy_input.trim().to_string();
                    app.worker.send(Cmd::SetOwnProxy(text));
                }

                // Check button
                let has_input = !app.own_proxy_input.trim().is_empty();
                let check_btn = egui::Button::new(
                    egui::RichText::new("Проверить").size(12.0).color(theme::TEXT),
                )
                .fill(theme::SUNKEN)
                .stroke(Stroke::new(1.0, theme::LINE))
                .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

                if ui.add_enabled(!app.proxy_test_running && has_input, check_btn).clicked() {
                    app.start_proxy_test(ui.ctx());
                }
            });
        });

        // Proxy Profiles / Presets
        let profiles = app.status.as_ref().map(|s| s.proxy_profiles.clone()).unwrap_or_default();
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Профили:")
                    .size(12.0)
                    .color(theme::MUTED),
            );

            let mut delete_idx: Option<usize> = None;
            let mut select_idx: Option<usize> = None;

            for (idx, prof) in profiles.iter().enumerate() {
                let is_selected = app.own_proxy_input.trim() == prof.address.trim();
                let prof_btn = egui::Button::new(
                    egui::RichText::new(&prof.name)
                        .size(11.5)
                        .color(if is_selected { egui::Color32::BLACK } else { theme::TEXT }),
                )
                .fill(if is_selected { egui::Color32::WHITE } else { theme::SUNKEN })
                .stroke(Stroke::new(1.0, theme::LINE))
                .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

                if ui.add_enabled(!busy, prof_btn).on_hover_text(&prof.address).clicked() {
                    select_idx = Some(idx);
                }

                let del_btn = egui::Button::new(egui::RichText::new("×").size(11.0).color(theme::MUTED))
                    .fill(theme::SUNKEN)
                    .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));
                if ui.add_enabled(!busy, del_btn).on_hover_text("Удалить профиль").clicked() {
                    delete_idx = Some(idx);
                }
            }

            if let Some(i) = select_idx {
                if let Some(p) = profiles.get(i) {
                    app.own_proxy_input = p.address.clone();
                }
                app.worker.send(Cmd::SelectProxyProfile(i));
            }
            if let Some(i) = delete_idx {
                app.worker.send(Cmd::DeleteProxyProfile(i));
            }

            let add_btn = egui::Button::new(
                egui::RichText::new(if app.show_add_profile { "− Скрыть" } else { "+ Сохранить как профиль" })
                    .size(11.5)
                    .color(theme::TEXT),
            )
            .fill(theme::SUNKEN)
            .stroke(Stroke::new(1.0, theme::LINE))
            .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

            if ui.add_enabled(!busy, add_btn).clicked() {
                app.show_add_profile = !app.show_add_profile;
            }
        });

        if app.show_add_profile {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Название:").size(12.0).color(theme::MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut app.new_profile_name)
                        .desired_width(140.0)
                        .hint_text("Нидерланды"),
                );
                let can_add = !app.new_profile_name.trim().is_empty() && !app.own_proxy_input.trim().is_empty();
                let save_prof_btn = egui::Button::new(egui::RichText::new("Добавить").size(12.0).color(theme::TEXT))
                    .fill(theme::SUNKEN)
                    .stroke(Stroke::new(1.0, theme::LINE))
                    .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

                if ui.add_enabled(!busy && can_add, save_prof_btn).clicked() {
                    let name = app.new_profile_name.trim().to_string();
                    let address = app.own_proxy_input.trim().to_string();
                    app.worker.send(Cmd::SaveProxyProfile { name, address });
                    app.new_profile_name.clear();
                    app.show_add_profile = false;
                }
            });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Own proxy toggle
        let mut own_proxy = app.status.as_ref().map(|s| s.get(Cap::OwnProxy).is_on()).unwrap_or(true);
        if widgets::switch_row(ui, &mut own_proxy, !busy, |ui| {
            ui.label(
                egui::RichText::new("Использовать указанный прокси")
                    .size(13.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.add_space(1.0);
            ui.label(
                egui::RichText::new("Направлять запросы Antigravity через этот прокси-сервер")
                    .size(11.5)
                    .color(theme::MUTED),
            );
        }) {
            if let Some(st) = &mut app.status {
                st.own_proxy = if own_proxy { crate::core::ops::State::On } else { crate::core::ops::State::Off };
            }
            app.worker.send(Cmd::Set(Cap::OwnProxy, own_proxy));
        }
    });
}

// ---------------------------------------------------------------------------
// Section 2: Antigravity Components
// ---------------------------------------------------------------------------

fn components_section(app: &mut App, ui: &mut egui::Ui) {
    let busy = app.is_busy();
    widgets::section_header(ui, "КОМПОНЕНТЫ ANTIGRAVITY");
    ui.add_space(4.0);

    widgets::card(ui, |ui| {
        if let Some(st) = &app.status {
            for (idx, row) in st.installs.iter().enumerate() {
                if idx > 0 {
                    ui.add_space(8.0);
                }

                ui.horizontal(|ui| {
                    let has_path = row.path.is_some();
                    let (dot_color, dot_symbol) = if has_path {
                        (theme::OK, "✓")
                    } else {
                        (theme::SUBTLE, "—")
                    };

                    ui.label(
                        egui::RichText::new(dot_symbol)
                            .color(dot_color)
                            .size(13.0)
                            .strong(),
                    );
                    ui.add_space(2.0);

                    // Allocate left text column with room reserved for right button
                    let text_w = (ui.available_width() - 84.0).max(100.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(text_w, 0.0),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.set_max_width(text_w);
                            ui.label(
                                egui::RichText::new(row.label)
                                    .size(13.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            let path_str = match &row.path {
                                Some(p) => mask_path(&p.display().to_string()),
                                None => "Не обнаружен в стандартных каталогах".to_string(),
                            };
                            ui.label(
                                egui::RichText::new(path_str)
                                    .size(11.5)
                                    .color(if has_path { theme::MUTED } else { theme::SUBTLE }),
                            );
                        },
                    );

                    // Browse button strictly aligned right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = egui::Button::new(
                            egui::RichText::new("Обзор").size(12.0).color(theme::TEXT),
                        )
                        .fill(theme::SUNKEN)
                        .stroke(Stroke::new(1.0, theme::LINE))
                        .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

                        if ui.add(btn).clicked() {
                            app.path_dialog = Some(String::new());
                        }
                    });
                });
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Watchdog toggle
        let mut watchdog = app.status.as_ref().map(|s| s.get(Cap::Watchdog).is_on()).unwrap_or(true);
        if widgets::switch_row(ui, &mut watchdog, !busy, |ui| {
            ui.label(
                egui::RichText::new("Автовосстановление при обновлениях (Watchdog)")
                    .size(13.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.add_space(1.0);
            ui.label(
                egui::RichText::new("Фоновое восстановление патчей при автообновлении IDE")
                    .size(11.5)
                    .color(theme::MUTED),
            );
        }) {
            if let Some(st) = &mut app.status {
                st.watchdog = if watchdog { crate::core::ops::State::On } else { crate::core::ops::State::Off };
            }
            app.worker.send(Cmd::Set(Cap::Watchdog, watchdog));
        }
    });
}

// ---------------------------------------------------------------------------
// Section 3: System & Autostart
// ---------------------------------------------------------------------------

fn system_section(app: &mut App, ui: &mut egui::Ui) {
    widgets::section_header(ui, "СИСТЕМА И ЗАПУСК");
    ui.add_space(4.0);

    widgets::card(ui, |ui| {
        // Autostart toggle
        if widgets::switch_row(ui, &mut app.autostart_enabled, true, |ui| {
            ui.label(
                egui::RichText::new("Автозапуск при старте системы")
                    .size(13.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.add_space(1.0);
            ui.label(
                egui::RichText::new("Запускать утилиту в системном трее при входе в систему")
                    .size(11.5)
                    .color(theme::MUTED),
            );
        }) {
            let res = crate::platform::autostart::set_enabled(app.autostart_enabled)
                .map(|_| "Настройки автозапуска сохранены".to_string());
            app.shortcut_status = Some((std::time::Instant::now(), res));
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        let desk_btn = egui::Button::new(
            egui::RichText::new("Создать ярлык на Рабочем столе").size(12.0).color(theme::TEXT),
        )
        .fill(theme::SUNKEN)
        .stroke(Stroke::new(1.0, theme::LINE))
        .corner_radius(CornerRadius::same(theme::RADIUS_SMALL))
        .min_size(egui::vec2(ui.available_width(), 28.0));

        if ui.add(desk_btn).clicked() {
            let res = crate::platform::shortcut::create_shortcuts();
            app.shortcut_status = Some((std::time::Instant::now(), res));
        }

        if let Some((at, res)) = &app.shortcut_status {
            if at.elapsed() < Duration::from_secs(8) {
                ui.add_space(6.0);
                match res {
                    Ok(msg) => {
                        ui.label(egui::RichText::new(format!("✓ {msg}")).size(12.0).color(theme::OK));
                    }
                    Err(e) => {
                        ui.label(egui::RichText::new(format!("✗ {e}")).size(12.0).color(theme::BAD));
                    }
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Logs Screen
// ---------------------------------------------------------------------------

fn logs_screen(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Журнал операций")
                .size(13.0)
                .strong()
                .color(theme::TEXT),
        );
        ui.label(
            egui::RichText::new(format!("{} записей", app.log.len()))
                .size(12.0)
                .color(theme::MUTED),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            let copy_btn = egui::Button::new(
                egui::RichText::new("Скопировать всё").size(11.5).color(theme::TEXT),
            )
            .fill(theme::SUNKEN)
            .stroke(Stroke::new(1.0, theme::LINE))
            .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

            if ui.add(copy_btn).clicked() {
                let text = app
                    .log
                    .iter()
                    .map(|(lvl, line)| format!("[{lvl:?}] {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.ctx().copy_text(text);
            }

            let clear_btn = egui::Button::new(
                egui::RichText::new("Очистить").size(11.5).color(theme::TEXT),
            )
            .fill(theme::SUNKEN)
            .stroke(Stroke::new(1.0, theme::LINE))
            .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

            if ui.add(clear_btn).clicked() {
                app.log.clear();
            }
        });
    });

    ui.add_space(6.0);

    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(10))
        .stroke(Stroke::new(1.0, theme::CARD_BORDER))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(340.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if app.log.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Журнал пуст")
                                    .size(13.0)
                                    .color(theme::SUBTLE),
                            );
                        });
                        ui.add_space(20.0);
                        return;
                    }

                    for (lvl, line) in &app.log {
                        let color = match lvl {
                            Level::Ok => theme::OK,
                            Level::Err => theme::BAD,
                            Level::Warn => theme::WARN,
                            _ => theme::MUTED,
                        };
                        ui.label(
                            egui::RichText::new(line)
                                .size(11.5)
                                .monospace()
                                .color(color),
                        );
                    }
                });
        });
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------

const FOOTER_TEXT: f32 = 12.0;
const REPORT_SHOWN_FOR: Duration = Duration::from_secs(30);

fn footer(app: &mut App, ui: &mut egui::Ui) {
    ui.spacing_mut().interact_size.y = 18.0;
    ui.spacing_mut().item_spacing.y = 0.0;

    let mut copy_report = false;
    let mut reveal_report: Option<std::path::PathBuf> = None;

    ui.horizontal(|ui| {
        let busy = app.is_busy();
        let report_btn = egui::Button::new(
            egui::RichText::new("📄 Экспорт отчёта").size(FOOTER_TEXT).color(theme::TEXT),
        )
        .fill(theme::SUNKEN)
        .stroke(Stroke::new(1.0, theme::LINE))
        .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

        if ui
            .add_enabled(!busy, report_btn)
            .on_hover_text("Сохранить диагностический отчёт для анализа неполадок")
            .clicked()
        {
            copy_report = true;
        }

        let saved = app
            .report_saved
            .clone()
            .filter(|(at, _)| at.elapsed() < REPORT_SHOWN_FOR);

        if let Some((_, path)) = &saved {
            ui.add_space(4.0);
            let show_btn = egui::Button::new(
                egui::RichText::new("📂 Показать файл").size(FOOTER_TEXT).color(theme::TEXT),
            )
            .fill(theme::SUNKEN)
            .stroke(Stroke::new(1.0, theme::LINE))
            .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

            if ui
                .add(show_btn)
                .on_hover_text("Открыть каталог с файлом отчёта")
                .clicked()
            {
                reveal_report = Some(path.clone());
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.hyperlink_to(
                egui::RichText::new("GitHub").size(FOOTER_TEXT),
                GITHUB_URL,
            );
            ui.label(egui::RichText::new("·").size(FOOTER_TEXT).color(theme::LINE));
            ui.hyperlink_to(
                egui::RichText::new("Сайт").size(FOOTER_TEXT),
                DOCS_URL,
            );
            ui.label(egui::RichText::new("·").size(FOOTER_TEXT).color(theme::LINE));
            let update_btn = egui::Button::new(
                egui::RichText::new(if app.manual_running {
                    "Проверка…"
                } else {
                    "Обновления"
                })
                .size(FOOTER_TEXT)
                .color(theme::TEXT),
            )
            .fill(theme::SUNKEN)
            .stroke(Stroke::new(1.0, theme::LINE))
            .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));

            if ui.add_enabled(!app.manual_running, update_btn).clicked() {
                app.check_update_manually(ui.ctx());
                crate::utils::open_url(GITHUB_RELEASES_URL);
            }
        });
    });

    if copy_report {
        let text = super::report::build(app.status.as_ref(), &app.gate);
        ui.ctx().copy_text(text.clone());
        let now = std::time::Instant::now();
        match crate::utils::report_dir()
            .and_then(|dir| crate::utils::save_text_file(&dir, crate::utils::REPORT_FILE, &text))
        {
            Some(path) => {
                app.report_saved = Some((now, path));
                app.report_clipboard_at = None;
            }
            None => {
                app.report_saved = None;
                app.report_clipboard_at = Some(now);
            }
        }
    }

    if let Some(path) = reveal_report {
        crate::utils::reveal_in_explorer(&path);
    }
}

// ---------------------------------------------------------------------------
// Path Dialog Modal
// ---------------------------------------------------------------------------

pub fn path_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut text) = app.path_dialog.take() else {
        return;
    };
    let mut keep_open = true;
    let mut submit = false;

    egui::Window::new("Путь к установке Antigravity")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(400.0);
            widgets::hint(
                ui,
                "Укажите путь к каталогу установки Antigravity IDE или CLI. \
                 Можно указать любой вложенный каталог — корень будет обнаружен автоматически.",
            );
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(f32::INFINITY)
                    .hint_text("C:\\Users\\...\\Programs\\Antigravity или /home/..."),
            );
            if let Some(err) = &app.path_dialog_error {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(err).color(theme::BAD).size(12.5));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if widgets::primary(ui, "Добавить", !text.trim().is_empty()).clicked() {
                    submit = true;
                }
                if widgets::ghost(ui, "Отмена").clicked() {
                    keep_open = false;
                }
            });
        });

    if submit {
        let cleaned = crate::clean_input_path(&text);
        match crate::ops::resolve_manual_path(std::path::Path::new(&cleaned)) {
            Some(root) => {
                app.worker.send(Cmd::AddPath(root));
                app.path_dialog_error = None;
                keep_open = false;
            }
            None => {
                app.path_dialog_error =
                    Some("По указанному пути установка Antigravity не обнаружена.".into());
            }
        }
    }

    if keep_open {
        app.path_dialog = Some(text);
    } else {
        app.path_dialog_error = None;
    }
}
