//! Modal dialogs: Path override dialog and Changelog modal.

use eframe::egui;

use crate::gui::widgets;
use crate::gui::{theme, App};
use crate::ops::Cmd;

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

pub fn changelog_dialog(app: &mut App, ctx: &egui::Context) {
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

