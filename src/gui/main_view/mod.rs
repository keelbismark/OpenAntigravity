//! The primary user interface: a clean, unified, native Control Center.

mod cards;
mod sections;
mod dialogs;

pub use dialogs::{path_dialog, changelog_dialog};


use eframe::egui::{self, CornerRadius, Sense, Stroke};
use std::time::Duration;

use super::status::{self, Tone};
use super::widgets::{self, Icon};
use super::{theme, App, Tab, DOCS_URL, GITHUB_URL};

pub fn view(app: &mut App, ui: &mut egui::Ui) {
    header(app, ui);
    ui.add_space(8.0);

    let footer_h = 58.0;
    let avail_h = (ui.available_height() - footer_h).max(120.0);

    if app.current_tab == Tab::Logs {
        sections::logs_screen(app, ui, avail_h);
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
            } else if let Some(f) = cards::facts(app) {
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
    cards::hero_status_card(app, ui);

    ui.add_space(14.0);
    cards::quick_proxy_card(app, ui);

    ui.add_space(14.0);
    sections::components_section(app, ui);

    ui.add_space(14.0);
    sections::system_section(app, ui);
}

fn proxy_screen(app: &mut App, ui: &mut egui::Ui) {
    sections::proxy_section(app, ui);
}

fn doctor_screen(app: &mut App, ui: &mut egui::Ui) {
    sections::doctor_section(app, ui);

    ui.add_space(14.0);
    sections::activity_inspector_section(app, ui);
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

