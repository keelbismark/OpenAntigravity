//! The primary user interface: a clean, unified, native Control Center.

use eframe::egui::{self, CornerRadius, Sense, Stroke};
use std::time::Duration;

use super::status::{self, Action, Tone};
use super::widgets::{self, Icon};
use super::{theme, App, Tab, DOCS_URL, GITHUB_URL};
use crate::ops::{Cap, Cmd, Level};
use crate::utils::mask_path;

pub fn view(app: &mut App, ui: &mut egui::Ui) {
    header(app, ui);
    ui.add_space(8.0);

    let footer_h = 58.0;
    let avail_h = (ui.available_height() - footer_h).max(120.0);

    if app.current_tab == Tab::Logs {
        logs_screen(app, ui, avail_h);
    } else {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(avail_h)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match app.current_tab {
                    Tab::Main => main_screen(app, ui),
                    Tab::Proxy => proxy_screen(app, ui),
                    Tab::Doctor => doctor_screen(app, ui),
                    Tab::Logs => unreachable!(),
                }
                ui.add_space(8.0);
            });
    }

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
        let galley = ui.painter().layout_no_wrap(ver_text, font_id.clone(), theme::MUTED);
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

        // Mode pill badge (Portable vs Installed)
        let (mode_text, mode_tooltip) = if crate::portable::enabled() {
            ("Портативная", "Портативный режим: настройки и данные хранятся в папке data/ рядом с программой")
        } else {
            ("Установленная", "Установленная версия: настройки и данные хранятся в системном каталоге (%LOCALAPPDATA%\\OpenAntigravity)")
        };
        let mode_galley = ui.painter().layout_no_wrap(mode_text.to_string(), font_id, theme::SUBTLE);
        let (mode_rect, mode_resp) = ui.allocate_exact_size(egui::vec2(mode_galley.size().x + 10.0, 18.0), Sense::hover());
        mode_resp.on_hover_text(mode_tooltip);
        if ui.is_rect_visible(mode_rect) {
            let painter = ui.painter();
            painter.rect(
                mode_rect,
                CornerRadius::same(theme::RADIUS_SMALL),
                theme::SUNKEN,
                Stroke::new(1.0, theme::LINE),
                egui::StrokeKind::Inside,
            );
            let text_pos = egui::pos2(mode_rect.center().x - mode_galley.size().x / 2.0, mode_rect.center().y - mode_galley.size().y / 2.0);
            painter.galley(text_pos, mode_galley, theme::SUBTLE);
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
    let options = [
        (Tab::Main, "Обзор"),
        (Tab::Proxy, "Прокси"),
        (Tab::Doctor, "Диагностика"),
        (Tab::Logs, &logs_label),
    ];
    widgets::site_tabs(ui, &mut app.current_tab, &options);
}

// ---------------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------------

fn main_screen(app: &mut App, ui: &mut egui::Ui) {
    hero_status_card(app, ui);

    ui.add_space(14.0);
    quick_proxy_card(app, ui);

    ui.add_space(14.0);
    components_section(app, ui);

    ui.add_space(14.0);
    system_section(app, ui);
}

fn proxy_screen(app: &mut App, ui: &mut egui::Ui) {
    proxy_section(app, ui);
}

fn doctor_screen(app: &mut App, ui: &mut egui::Ui) {
    doctor_section(app, ui);

    ui.add_space(14.0);
    activity_inspector_section(app, ui);
}

fn quick_proxy_card(app: &mut App, ui: &mut egui::Ui) {
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

// ---------------------------------------------------------------------------
// Section: Doctor / Environment Diagnostics
// ---------------------------------------------------------------------------

fn doctor_section(app: &mut App, ui: &mut egui::Ui) {
    let busy = app.is_busy();

    ui.horizontal(|ui| {
        widgets::section_header(ui, "ПРОВЕРКА СИСТЕМЫ");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let running = app.diag_running || app.proxy_test_running;
            let btn_text = if running { "Тестирование…" } else { "Проверить всё" };
            let run_btn_resp = widgets::btn(
                ui,
                Some(Icon::Refresh { spin: running }),
                btn_text,
                11.5,
                None,
            );

            if !busy && !running && run_btn_resp.clicked() {
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
                "Редактор Antigravity",
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
                "Связь с Google AI",
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

        let mut fix_to_run: Option<crate::diag::AutoFixAction> = None;

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

        if let Some(report) = &app.last_diag_report {
            let fixes: Vec<(&crate::diag::DiagItem, crate::diag::AutoFixAction)> = report
                .items
                .iter()
                .filter_map(|it| it.fix.map(|f| (it, f)))
                .collect();

            if !fixes.is_empty() {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);

                ui.label(
                    egui::RichText::new("Рекомендованные исправления:")
                        .size(12.0)
                        .strong()
                        .color(egui::Color32::from_rgb(235, 175, 45)),
                );
                ui.add_space(4.0);

                for (item, fix_action) in fixes {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("• {}: {}", item.name, item.summary))
                                .size(11.5)
                                .color(theme::TEXT),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let fix_btn = widgets::btn(ui, Some(Icon::Bolt), fix_action.label(), 11.0, None);
                            if !busy && fix_btn.clicked() {
                                fix_to_run = Some(fix_action);
                            }
                        });
                    });
                }
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Сетевые инструменты:")
                    .size(11.5)
                    .color(theme::MUTED),
            );

            let dns_resp = widgets::btn(ui, Some(Icon::Refresh { spin: false }), "Сброс DNS-кэша", 11.0, None)
                .on_hover_text("Очистить системный кэш DNS для мгновенного применения сетевых настроек");
            if !busy && dns_resp.clicked() {
                fix_to_run = Some(crate::diag::AutoFixAction::FlushDns);
            }

            #[cfg(windows)]
            {
                let nat_resp = widgets::btn(ui, Some(Icon::Bolt), "Перезапуск WinNAT", 11.0, None)
                    .on_hover_text("Перезапустить службу сетевой трансляции Windows (WinNAT) для освобождения системных портов");
                if !busy && nat_resp.clicked() {
                    fix_to_run = Some(crate::diag::AutoFixAction::RestartWinNat);
                }
            }
        });

        if let Some(action) = fix_to_run {
            app.worker.send(Cmd::AutoFix(action));
        }
    });
}

// ---------------------------------------------------------------------------
// Section: AI Activity Inspector (Live Gemini & IDE Telemetry)
// ---------------------------------------------------------------------------

fn activity_inspector_section(app: &mut App, ui: &mut egui::Ui) {
    widgets::section_header(ui, "ЖИВЫЕ ЗАПРОСЫ К ИИ");
    ui.add_space(4.0);

    widgets::card(ui, |ui| {
        let summary = app.activity_summary.clone().unwrap_or_default();

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            let total_str = format!("Всего: {}", summary.total_turns);
            widgets::metric_chip(ui, Some(Icon::Bolt), &total_str);

            let ok_str = format!("Успешно: {}", summary.success_turns);
            widgets::metric_chip(ui, Some(Icon::Check), &ok_str);

            if summary.blocked_turns > 0 {
                let blocked_str = format!("Заблокировано: {}", summary.blocked_turns);
                widgets::metric_chip(ui, Some(Icon::Cross), &blocked_str);
            }

            if let Some(ago) = summary.newest_event_ago {
                let secs = ago.as_secs();
                let ago_str = if secs < 60 {
                    format!("{secs} с назад")
                } else {
                    format!("{} мин назад", secs / 60)
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("Активность: {ago_str}"))
                            .size(11.5)
                            .color(theme::MUTED),
                    );
                });
            }
        });

        if summary.recent_events.is_empty() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Ожидание запросов от редактора. Отправьте сообщение в чат IDE для проверки.")
                        .size(11.5)
                        .color(theme::SUBTLE),
                );
            });
        } else {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            for (idx, ev) in summary.recent_events.iter().take(5).enumerate() {
                if idx > 0 {
                    ui.add_space(4.0);
                }
                ui.horizontal(|ui| {
                    let (status_text, status_col) = match ev.status {
                        crate::core::ls_log::TurnStatus::Ok200 => ("200 OK", theme::OK),
                        crate::core::ls_log::TurnStatus::Refusal400 => ("400 BLOCKED", theme::BAD),
                        crate::core::ls_log::TurnStatus::Error => ("ERR", theme::WARN),
                    };

                    ui.label(
                        egui::RichText::new(status_text)
                            .size(11.0)
                            .strong()
                            .color(status_col),
                    );

                    ui.label(
                        egui::RichText::new(&ev.model)
                            .size(11.5)
                            .strong()
                            .color(theme::TEXT),
                    );

                    if let Some(ref resp_id) = ev.response_id {
                        ui.label(
                            egui::RichText::new(resp_id)
                                .size(11.0)
                                .monospace()
                                .color(theme::SUBTLE),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let time_str = if let Some(ago) = ev.ago_secs {
                            if ago < 60 {
                                format!("{ago} с назад")
                            } else {
                                format!("{} мин назад", ago / 60)
                            }
                        } else {
                            ev.time_hms.clone()
                        };
                        ui.label(
                            egui::RichText::new(time_str)
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    });
                });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Section 1: Proxy Configuration
// ---------------------------------------------------------------------------

fn proxy_section(app: &mut App, ui: &mut egui::Ui) {
    let busy = app.is_busy();
    widgets::section_header(ui, "ПАРАМЕТРЫ ПРОКСИ");
    ui.add_space(4.0);

    widgets::card(ui, |ui| {
        ui.label(
            egui::RichText::new("Основной адрес (HTTP / SOCKS5)")
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
                let save_resp = widgets::btn(ui, Some(Icon::Check), "Сохранить", 12.0, None);
                if !busy && save_resp.clicked() {
                    let text = app.own_proxy_input.trim().to_string();
                    app.worker.send(Cmd::SetOwnProxy(text));
                }

                // Check button
                let has_input = !app.own_proxy_input.trim().is_empty();
                let check_label = if app.proxy_test_running { "Замер…" } else { "Проверить" };
                let check_resp = widgets::btn(
                    ui,
                    Some(Icon::Refresh { spin: app.proxy_test_running }),
                    check_label,
                    12.0,
                    None,
                );

                if !app.proxy_test_running && has_input && check_resp.clicked() {
                    app.start_proxy_test(ui.ctx());
                }
            });
        });

        // Multi-proxy ranking & status
        let ranked = crate::upstream::get_ranked_upstreams();
        if ranked.len() > 1 {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Резервные узлы:")
                        .size(11.5)
                        .color(theme::MUTED),
                );
                for up in ranked.iter().take(3) {
                    let score_str = if let Some(score) = crate::upstream::get_upstream_score(up) {
                        if let Some(rtt) = score.rtt_ms {
                            format!("{} ({} мс)", up.display(), rtt)
                        } else if score.err.is_some() {
                            format!("{} (сбой)", up.display())
                        } else {
                            up.display()
                        }
                    } else {
                        up.display()
                    };
                    widgets::metric_chip(ui, Some(Icon::Bolt), &score_str);
                }
            });
        }

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
                let prof_resp = widgets::profile_pill(ui, &prof.name, is_selected, !busy);
                if prof_resp.on_hover_text(&prof.address).clicked() {
                    select_idx = Some(idx);
                }

                let del_btn = egui::Button::new(egui::RichText::new("×").size(11.0).color(theme::MUTED))
                    .fill(theme::SUNKEN)
                    .stroke(Stroke::new(1.0, theme::LINE))
                    .corner_radius(CornerRadius::same(theme::RADIUS_SMALL));
                if ui
                    .add_enabled(!busy, del_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Удалить профиль")
                    .clicked()
                {
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

            let (add_icon, add_label) = if app.show_add_profile {
                (Icon::Cross, "Скрыть")
            } else {
                (Icon::Plus, "Сохранить как профиль")
            };
            let add_resp = widgets::btn(ui, Some(add_icon), add_label, 11.5, None);
            if !busy && add_resp.clicked() {
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
                let save_prof_resp = widgets::btn(ui, Some(Icon::Plus), "Добавить", 12.0, None);

                if !busy && can_add && save_prof_resp.clicked() {
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
                egui::RichText::new("Использовать этот прокси")
                    .size(13.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.add_space(1.0);
            ui.label(
                egui::RichText::new("Направлять трафик Language Server через указанный адрес")
                    .size(11.5)
                    .color(theme::MUTED),
            );
        }) {
            if let Some(st) = &mut app.status {
                st.own_proxy = if own_proxy { crate::core::ops::State::On } else { crate::core::ops::State::Off };
            }
            app.worker.send(Cmd::Set(Cap::OwnProxy, own_proxy));
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        ui.label(
            egui::RichText::new("Терминал и консоль (CLI)")
                .size(12.5)
                .strong()
                .color(theme::TEXT),
        );
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("Экспорт переменных HTTP_PROXY для curl, git, pip через локальный шлюз:")
                .size(11.5)
                .color(theme::MUTED),
        );
        ui.add_space(6.0);

        let port = crate::proxy::port();
        ui.horizontal(|ui| {
            let ps_resp = widgets::btn(ui, Some(Icon::Copy), "PowerShell", 11.5, None)
                .on_hover_text("Скопировать команду экспорта переменных для PowerShell ($env:HTTP_PROXY=...)");
            if ps_resp.clicked() {
                ui.ctx().copy_text(crate::utils::proxy_env_powershell(port));
                app.terminal_env_copied = Some(("PowerShell".to_string(), std::time::Instant::now()));
            }

            let cmd_resp = widgets::btn(ui, Some(Icon::Copy), "CMD", 11.5, None)
                .on_hover_text("Скопировать команду экспорта переменных для командной строки Windows (set HTTP_PROXY=...)");
            if cmd_resp.clicked() {
                ui.ctx().copy_text(crate::utils::proxy_env_cmd(port));
                app.terminal_env_copied = Some(("CMD".to_string(), std::time::Instant::now()));
            }

            let bash_resp = widgets::btn(ui, Some(Icon::Copy), "Bash / Zsh", 11.5, None)
                .on_hover_text("Скопировать команду экспорта переменных для Linux/macOS Bash и Zsh (export HTTP_PROXY=...)");
            if bash_resp.clicked() {
                ui.ctx().copy_text(crate::utils::proxy_env_bash(port));
                app.terminal_env_copied = Some(("Bash/Zsh".to_string(), std::time::Instant::now()));
            }

            let run_resp = widgets::btn(ui, Some(Icon::Desktop), "Открыть терминал", 11.5, None)
                .on_hover_text("Открыть окно терминала с автоматически настроенными переменными прокси");
            if run_resp.clicked() {
                let _ = crate::utils::launch_terminal_with_proxy(port);
            }
        });

        if let Some((shell, at)) = &app.terminal_env_copied {
            if at.elapsed() < std::time::Duration::from_secs(4) {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("✓ Команда для {} скопирована в буфер обмена!", shell))
                            .color(theme::OK)
                            .size(11.5)
                            .strong(),
                    );
                });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Section 2: Antigravity Components
// ---------------------------------------------------------------------------

fn components_section(app: &mut App, ui: &mut egui::Ui) {
    let busy = app.is_busy();
    widgets::section_header(ui, "УСТАНОВКИ IDE");
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
                        let browse_resp = widgets::btn(ui, Some(Icon::Folder), "Обзор", 12.0, None);
                        if browse_resp.clicked() {
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
                egui::RichText::new("Автопатч при обновлениях")
                    .size(13.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.add_space(1.0);
            ui.label(
                egui::RichText::new("Автоматически возвращать патч после обновления IDE")
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
    widgets::section_header(ui, "СИСТЕМА");
    ui.add_space(4.0);

    widgets::card(ui, |ui| {
        // Autostart toggle
        if widgets::switch_row(ui, &mut app.autostart_enabled, true, |ui| {
            ui.label(
                egui::RichText::new("Запуск вместе с системой")
                    .size(13.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.add_space(1.0);
            ui.label(
                egui::RichText::new("Фоновый запуск в трее при входе в систему")
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

        let desk_resp = widgets::btn(
            ui,
            Some(Icon::Desktop),
            "Создать ярлык на рабочем столе",
            12.0,
            Some(egui::vec2(ui.available_width(), 28.0)),
        );

        if desk_resp.clicked() {
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

fn logs_screen(app: &mut App, ui: &mut egui::Ui, avail_h: f32) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("События и логи")
                .size(13.0)
                .strong()
                .color(theme::TEXT),
        );
        ui.label(
            egui::RichText::new(format!("{} строк", app.log.len()))
                .size(12.0)
                .color(theme::MUTED),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            let clear_resp = widgets::btn(ui, Some(Icon::Trash), "Очистить", 11.5, None);
            if clear_resp.clicked() {
                app.log.clear();
            }

            let copied = app
                .logs_copied_at
                .map(|t| t.elapsed() < Duration::from_secs(3))
                .unwrap_or(false);
            let (copy_icon, copy_label) = if copied {
                (Icon::Check, "Скопировано!")
            } else {
                (Icon::Copy, "Скопировать")
            };

            let copy_resp = widgets::btn(ui, Some(copy_icon), copy_label, 11.5, None);
            if copy_resp.clicked() {
                let text = app
                    .log
                    .iter()
                    .map(|(lvl, line)| format!("[{lvl:?}] {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.ctx().copy_text(text);
                app.logs_copied_at = Some(std::time::Instant::now());
            }
        });
    });

    ui.add_space(6.0);

    let card_h = (avail_h - 36.0).max(180.0);

    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(10))
        .stroke(Stroke::new(1.0, theme::CARD_BORDER))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let inner_h = (card_h - 20.0).max(160.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(inner_h)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if app.log.is_empty() {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Журнал пуст")
                                    .size(13.0)
                                    .color(theme::SUBTLE),
                            );
                        });
                        ui.add_space(40.0);
                        return;
                    }

                    for (lvl, line) in &app.log {
                        let color = match lvl {
                            Level::Ok => theme::OK,
                            Level::Err => theme::BAD,
                            Level::Warn => theme::WARN,
                            Level::Step => theme::TEXT,
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
        let saved = app
            .report_saved
            .clone()
            .filter(|(at, _)| at.elapsed() < REPORT_SHOWN_FOR);

        if let Some((_, path)) = &saved {
            let show_resp = widgets::btn(ui, Some(Icon::Folder), "Показать отчёт", FOOTER_TEXT, None)
                .on_hover_text("Открыть каталог с файлом отчёта в проводнике");

            if show_resp.clicked() {
                reveal_report = Some(path.clone());
            }
        } else {
            let report_resp = widgets::btn(ui, Some(Icon::Export), "Экспорт отчёта", FOOTER_TEXT, None)
                .on_hover_text("Сохранить диагностический отчёт для анализа неполадок");

            if !busy && report_resp.clicked() {
                copy_report = true;
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

            let (update_label, update_tooltip) = if app.manual_running {
                ("Проверка…", Some("Проверка наличия обновлений на сервере…".to_string()))
            } else if let Some((label, tip, _)) = &app.manual_result {
                (label.as_str(), Some(tip.clone()))
            } else {
                ("Проверить обновления", Some("Проверить наличие новой версии программы".to_string()))
            };

            let mut update_resp = widgets::btn(
                ui,
                Some(Icon::Update),
                update_label,
                FOOTER_TEXT,
                None,
            );
            if let Some(tip) = update_tooltip {
                update_resp = update_resp.on_hover_text(tip);
            }

            if !app.manual_running && update_resp.clicked() {
                app.check_update_manually(ui.ctx());
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

pub(super) fn changelog_dialog(app: &mut App, ctx: &egui::Context) {
    if !app.show_changelog_modal {
        return;
    }
    let Some(rel) = app.update.clone() else {
        app.show_changelog_modal = false;
        return;
    };

    let mut keep_open = true;
    let mut trigger_update = false;
    let mut open_browser = false;

    egui::Window::new(format!("Что нового в версии {}", rel.display_version()))
        .collapsible(false)
        .resizable(true)
        .default_width(520.0)
        .default_height(380.0)
        .min_width(380.0)
        .min_height(260.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                widgets::show_icon(ui, widgets::Icon::Update, theme::ACCENT, 18.0);
                ui.label(
                    egui::RichText::new(format!("Open Antigravity v{}", rel.display_version()))
                        .size(16.0)
                        .strong()
                        .color(theme::TEXT),
                );
                if let Some(date) = &rel.date {
                    ui.label(egui::RichText::new(format!("от {date}")).size(12.0).color(theme::MUTED));
                }
            });

            ui.add_space(6.0);

            // Mode & SHA-256 badges
            ui.horizontal(|ui| {
                let mode_label = if crate::portable::enabled() {
                    "Сборка: Портативная"
                } else {
                    "Сборка: Установленная"
                };
                let mode_badge = egui::Frame::NONE
                    .fill(theme::SUNKEN)
                    .stroke(egui::Stroke::new(1.0, theme::CARD_BORDER))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_SMALL))
                    .inner_margin(egui::Margin::symmetric(8, 4));
                mode_badge.show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(mode_label)
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                });
                if let Some(hash) = rel.expected_sha256() {
                    let short_hash = if hash.len() > 16 {
                        format!("{}...{}", &hash[..8], &hash[hash.len() - 8..])
                    } else {
                        hash.to_string()
                    };
                    let badge = egui::Frame::NONE
                        .fill(theme::SUNKEN)
                        .stroke(egui::Stroke::new(1.0, theme::CARD_BORDER))
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_SMALL))
                        .inner_margin(egui::Margin::symmetric(8, 4));
                    badge.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            widgets::show_icon(ui, widgets::Icon::Check, theme::OK, 12.0);
                            ui.label(
                                egui::RichText::new(format!("SHA-256: {short_hash}"))
                                    .size(11.0)
                                    .color(theme::MUTED),
                            )
                            .on_hover_text(format!("Полная контрольная сумма:\n{hash}\n\nЦелостность файла проверяется перед установкой."));
                        });
                    });
                } else {
                    let badge = egui::Frame::NONE
                        .fill(theme::SUNKEN)
                        .stroke(egui::Stroke::new(1.0, theme::CARD_BORDER))
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_SMALL))
                        .inner_margin(egui::Margin::symmetric(8, 4));
                    badge.show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("SHA-256: динамическая проверка")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    });
                }
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Scrollable changelog notes
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(notes) = &rel.notes {
                        for line in notes.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("### ") {
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(&trimmed[4..])
                                        .size(13.0)
                                        .strong()
                                        .color(theme::ACCENT),
                                );
                                ui.add_space(2.0);
                            } else if trimmed.starts_with("## ") {
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new(&trimmed[3..])
                                        .size(14.0)
                                        .strong()
                                        .color(theme::TEXT),
                                );
                                ui.add_space(4.0);
                            } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("•").size(12.0).color(theme::ACCENT));
                                    ui.label(egui::RichText::new(&trimmed[2..]).size(12.0).color(theme::TEXT));
                                });
                            } else if trimmed.is_empty() {
                                ui.add_space(4.0);
                            } else {
                                ui.label(egui::RichText::new(trimmed).size(12.0).color(theme::TEXT));
                            }
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("Описание изменений для этой версии не предоставлено.")
                                .size(12.0)
                                .color(theme::MUTED),
                        );
                    }
                });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);

            // Action buttons
            ui.horizontal(|ui| {
                if widgets::primary(ui, "Обновить сейчас", true).clicked() {
                    trigger_update = true;
                    keep_open = false;
                }
                if let Some(setup_url) = &rel.url_windows_setup {
                    if !crate::portable::enabled() && widgets::ghost(ui, "Скачать установщик (.exe)").clicked() {
                        crate::utils::open_url(setup_url);
                    }
                }
                if widgets::ghost(ui, "Открыть на GitHub").clicked() {
                    open_browser = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::ghost(ui, "Закрыть").clicked() {
                        keep_open = false;
                    }
                });
            });
        });

    if !keep_open {
        app.show_changelog_modal = false;
    }
    if open_browser {
        if let Some(url) = rel.landing_url() {
            crate::utils::open_url(url);
        }
    }
    if trigger_update {
        app.start_auto_update(&rel, ctx);
    }
}

