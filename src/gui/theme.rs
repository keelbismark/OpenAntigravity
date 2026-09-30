//! One place for every colour, radius and spacing the GUI uses.
//!
//! The design brief was "simple, buttons": one dark surface, one accent, and a
//! green/red pair that only ever means on/off. Anything that needs a *fourth*
//! colour is a sign the screen is doing too much.

use eframe::egui::{self, Color32, CornerRadius, Stroke};

/// Window background — deep black (same as site --bg: #000000).
pub const BG: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x0C);
/// A card: obsidian surface with subtle glow (site --card-bg).
pub const CARD: Color32 = Color32::from_rgb(0x11, 0x11, 0x14);
/// Card stroke: subtle outline (site --card-border).
pub const CARD_BORDER: Color32 = Color32::from_rgb(0x22, 0x22, 0x27);
/// A control inside a card (text field, inactive switch track, charts).
pub const SUNKEN: Color32 = Color32::from_rgb(0x17, 0x17, 0x1B);
/// Hairline between rows and subtle borders.
pub const LINE: Color32 = Color32::from_rgb(0x20, 0x20, 0x25);

/// Primary text (crisp white).
pub const TEXT: Color32 = Color32::from_rgb(0xF8, 0xFA, 0xFC);
/// Secondary text: descriptions, paths, hints (Slate-400).
pub const MUTED: Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);
/// Very subtle text.
pub const SUBTLE: Color32 = Color32::from_rgb(0x64, 0x74, 0x8B);

/// The accent (crisp White like site's primary CTA).
pub const ACCENT: Color32 = Color32::WHITE;
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xE2, 0xE8, 0xF0);
pub const ACCENT_SUBTLE: Color32 = Color32::from_rgb(0x27, 0x27, 0x2A);

/// "On / succeeded" (Emerald).
pub const OK: Color32 = Color32::from_rgb(0x22, 0xC5, 0x5E);
pub const OK_SUBTLE: Color32 = Color32::from_rgb(0x05, 0x2E, 0x16);
/// "Needs attention" (Amber).
pub const WARN: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
/// "Off / failed" (Rose/Coral).
pub const BAD: Color32 = Color32::from_rgb(0xEF, 0x44, 0x44);

pub const RADIUS: u8 = 12;
pub const RADIUS_SMALL: u8 = 7;
pub const RADIUS_PILL: u8 = 99;

/// Installs a clean system UI font when available.
fn install_font(ctx: &egui::Context) {
    #[cfg(target_os = "windows")]
    {
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
        let fonts_dir = std::path::Path::new(&dir).join("Fonts");
        let candidates = ["seguisb.ttf", "segoeui.ttf"];
        for f in candidates {
            let path = fonts_dir.join(f);
            if let Ok(bytes) = std::fs::read(&path) {
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert(
                    f.to_string(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );

                // Segoe UI is purely alphanumeric/Cyrillic without checkmarks or symbols.
                // Load Segoe UI Symbol as fallback for ✓ (U+2713), ✗ (U+2717), etc.
                let sym_file = "seguisym.ttf";
                let sym_path = fonts_dir.join(sym_file);
                if let Ok(sym_bytes) = std::fs::read(&sym_path) {
                    fonts.font_data.insert(
                        sym_file.to_string(),
                        std::sync::Arc::new(egui::FontData::from_owned(sym_bytes)),
                    );
                }

                let prop = fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default();

                if fonts.font_data.contains_key(sym_file) {
                    prop.insert(0, sym_file.to_string());
                }
                prop.insert(0, f.to_string());
                ctx.set_fonts(fonts);
                return;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let linux_candidates = [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/inter/Inter-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
        ];
        for path_str in linux_candidates {
            let path = std::path::Path::new(path_str);
            if let Ok(bytes) = std::fs::read(path) {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("custom");
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert(
                    name.to_string(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, name.to_string());
                ctx.set_fonts(fonts);
                return;
            }
        }
    }
}

/// Applies the theme to a context. Called once at startup.
///
/// egui ships Ubuntu-Light and Hack as its default fonts and both carry the
/// Cyrillic block, so a Russian UI renders even when `install_font` finds
/// nothing — verified by running the licence screen, not assumed.
pub fn apply(ctx: &egui::Context) {
    install_font(ctx);

    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = SUNKEN;
    visuals.faint_bg_color = CARD;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;

    let r = CornerRadius::same(RADIUS_SMALL);
    visuals.widgets.noninteractive.bg_fill = CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    visuals.widgets.noninteractive.corner_radius = r;

    visuals.widgets.inactive.bg_fill = SUNKEN;
    visuals.widgets.inactive.weak_bg_fill = SUNKEN;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    visuals.widgets.inactive.corner_radius = r;

    visuals.widgets.hovered.bg_fill = LINE;
    visuals.widgets.hovered.weak_bg_fill = LINE;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.corner_radius = r;

    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_HOVER);
    visuals.widgets.active.corner_radius = r;

    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.4);
    visuals.selection.stroke = Stroke::new(1.0, TEXT);

    // Pinned to dark rather than following the OS: every colour above was picked
    // against BG, and half a theme is worse than the wrong one.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);

    ctx.all_styles_mut(|style| {
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Heading, FontId::new(21.0, Proportional)),
            (TextStyle::Body, FontId::new(14.5, Proportional)),
            (TextStyle::Button, FontId::new(14.5, Proportional)),
            (TextStyle::Small, FontId::new(12.5, Proportional)),
            (
                TextStyle::Monospace,
                FontId::new(13.0, egui::FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size.y = 26.0;
        // Always visible and taking its own column, not floating over the
        // content: a bar that only appears on hover leaves no sign that there is
        // more below, and this window is taller than it fits.
        style.spacing.scroll.floating = false;
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.bar_inner_margin = 2.0;
        style.spacing.scroll.bar_outer_margin = 0.0;
    });
}
