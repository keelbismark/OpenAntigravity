//! Sections of the Control Center: Doctor diagnostics, Proxy settings, Activity inspector, Components, System, and Logs screen.

use eframe::egui::{self, CornerRadius, Stroke};
use std::time::Duration;

use crate::gui::widgets::{self, Icon};
use crate::gui::{theme, App};
use crate::ops::{Cap, Cmd, Level};
use crate::utils::mask_path;

// ---------------------------------------------------------------------------
// Section: Doctor / Environment Diagnostics
// ---------------------------------------------------------------------------

pub(super) fn doctor_section(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn activity_inspector_section(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn proxy_section(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn components_section(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn system_section(app: &mut App, ui: &mut egui::Ui) {
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

pub(super) fn logs_screen(app: &mut App, ui: &mut egui::Ui, avail_h: f32) {
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

