//! The desktop GUI.
//!
//! Replaces the numbered console menu. Two screens: a licence gate, then one
//! window of switches. Nothing here blocks — everything that touches the system
//! goes to the `ops` worker and comes back as status snapshots and log lines.

mod icon;
mod main_view;
pub(crate) mod renderer;
pub(crate) mod report;
pub(crate) mod status;
mod theme;
mod tray;
mod widgets;

use eframe::egui;
use std::sync::mpsc::{channel, Receiver};

use crate::gate;
use crate::ops::{self, Cmd, Event, Level, Status, Worker};
use crate::settings::Settings;
use crate::update::{self, UpdateInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Main,
    Logs,
}

pub const DOCS_URL: &str = "https://keelbismark.github.io/OpenAntigravity";
pub const GITHUB_URL: &str = "https://github.com/keelbismark/OpenAntigravity";
pub const GITHUB_RELEASES_URL: &str = "https://github.com/keelbismark/OpenAntigravity/releases";

const WIN_W: f32 = 480.0;
const WIN_H: f32 = 680.0;

/// How many log lines are kept. The window can stay open for days; an unbounded
/// Vec of every line a re-patch loop produced is a slow leak.
const LOG_LIMIT: usize = 400;

pub struct App {
    /// Raised once the feed reports a newer version and never lowered — the
    /// banner has to survive across screens and updates.
    update: Option<UpdateInfo>,
    update_rx: Receiver<UpdateInfo>,

    /// The «Проверить обновления» button: its sender goes to the one-shot
    /// thread, the receiver drains here, and the result is shown in the
    /// footer until it expires.
    manual_tx: std::sync::mpsc::Sender<Result<Option<UpdateInfo>, String>>,
    manual_rx: Receiver<Result<Option<UpdateInfo>, String>>,
    manual_running: bool,
    manual_result: Option<(String, std::time::Instant)>,

    worker: Worker,
    events: Receiver<Event>,
    status: Option<Status>,

    /// What the gate watcher last found: the client's own region-400s, and the
    /// relay's record of what it did about them.
    gate: gate::View,
    /// When that arrived. The watcher sends an age measured at its own tick and
    /// then stays quiet while nothing changes (waking the UI three times a
    /// minute to redraw the same line is not worth it), so the window ages its
    /// copy itself from here.
    gate_at: std::time::Instant,
    gate_rx: Receiver<gate::Signal>,
    log: Vec<(Level, String)>,
    /// Ctrl+A over the journal. Our own, because egui's label selection is per
    /// galley and the journal is one label per line.
    log_all_selected: bool,
    busy: Option<String>,

    own_proxy_input: String,
    /// The provider list as the window is drawing it right now.
    ///
    /// Kept beside the worker's snapshot so a drag can reorder it on the spot.
    /// Waiting for the round trip — save, re-read, push a new status — makes the
    /// row snap back under the pointer and the drag feel broken, which is
    /// exactly what it looked like.
    providers_local: Vec<crate::ops::ProviderRow>,
    /// True between picking a row up and the worker acknowledging the new order.
    providers_reordering: bool,
    path_dialog: Option<String>,
    path_dialog_error: Option<String>,
    /// The report file the button last wrote, and when — for as long as the
    /// card keeps telling the user where it is. Longer than a toast on purpose:
    /// this is an instruction to go and attach a file, not an acknowledgement.
    report_saved: Option<(std::time::Instant, std::path::PathBuf)>,
    /// Set instead when there was nowhere to write it and the text went to the
    /// clipboard alone.
    report_clipboard_at: Option<std::time::Instant>,
    shortcut_status: Option<(std::time::Instant, Result<String, String>)>,
    diag_tx: std::sync::mpsc::Sender<crate::diag::DiagReport>,
    diag_rx: Receiver<crate::diag::DiagReport>,
    diag_running: bool,
    diag_summary: Option<(std::time::Instant, crate::diag::DiagStatus, String)>,
    proxy_test_tx: std::sync::mpsc::Sender<Result<(u128, Option<String>), String>>,
    proxy_test_rx: Receiver<Result<(u128, Option<String>), String>>,
    pub proxy_test_running: bool,
    pub proxy_test_result: Option<(Result<(u128, Option<String>), String>, std::time::Instant)>,
    /// The system-tray icon.  Dropping the value removes the icon from the OS
    /// notification area, so this field lives as long as the window.
    _tray_icon: Option<tray_icon::TrayIcon>,
    /// Menu items whose IDs are matched in `drain_tray_events`.
    tray_items: Option<tray::TrayItems>,
    /// True while the window is hidden (minimised to tray).
    tray_hidden: bool,
    /// Whether autostart with system is enabled.
    pub autostart_enabled: bool,
    /// History of proxy ping / latency samples (ms) for the GUI sparkline chart.
    pub latency_history: Vec<u32>,
    /// Last measured RTT (ms).
    pub last_rtt: Option<u32>,
    /// Active tab in the UI.
    pub current_tab: Tab,
    /// Last received diagnostic report for Doctor view.
    pub last_diag_report: Option<crate::diag::DiagReport>,
    /// Buffer for creating a new proxy profile name.
    pub new_profile_name: String,
    /// UI toggle for adding a new profile.
    pub show_add_profile: bool,
    /// Last time an automatic background ping check ran.
    pub last_ping_time: Option<std::time::Instant>,
    frames: u8,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let (up_tx, update_rx) = channel();
        let up_ctx = cc.egui_ctx.clone();
        update::spawn_watch(up_tx, Box::new(move || up_ctx.request_repaint()));

        let (manual_tx, manual_rx) = channel();
        let (diag_tx, diag_rx) = channel();
        let (proxy_test_tx, proxy_test_rx) = channel();

        // The worker wakes the UI itself: egui sleeps until something asks it to
        // repaint, so an event that only lands in a channel is an event the user
        // never sees.
        let (ev_tx, events) = channel();
        let ctx = cc.egui_ctx.clone();
        let worker = ops::spawn(ev_tx, Box::new(move || ctx.request_repaint()));

        // Its own thread and its own channel rather than a command on the
        // worker's queue: the worker can be minutes deep in a patch run, and
        // "is Antigravity hitting the gate right now" is exactly the question
        // that must still answer while it is.
        let (gate_tx, gate_rx) = channel();
        let gate_ctx = cc.egui_ctx.clone();
        gate::spawn_watch(gate_tx, Box::new(move || gate_ctx.request_repaint()));

        // Read once, only to pre-fill the two fields. The worker owns the file
        // from here on — two writers each saving the whole thing meant whichever
        // saved last silently reverted the other.
        let settings = Settings::load();
        worker.send(Cmd::Unlocked);

        // System tray: best-effort.  If the OS has no notification daemon or
        // D-Bus is down (headless server, Wayland-only without SNI host), the
        // app works normally — just without the tray icon.
        let (tray_icon, tray_items) = match tray::build() {
            Some((icon, items)) => {
                tray::install_wakeup(&cc.egui_ctx);
                (Some(icon), Some(items))
            }
            None => (None, None),
        };

        let start_hidden = std::env::args().any(|a| a == "--minimized" || a == "-m" || a == "--tray");
        let autostart_enabled = crate::platform::autostart::is_enabled();

        Self {
            update: None,
            update_rx,
            manual_tx,
            manual_rx,
            manual_running: false,
            manual_result: None,
            worker,
            events,
            status: None,
            gate: gate::View::default(),
            gate_at: std::time::Instant::now(),
            gate_rx,
            log: Vec::new(),
            log_all_selected: false,
            busy: None,
            own_proxy_input: settings.own_proxy.clone(),
            providers_local: Vec::new(),
            providers_reordering: false,
            path_dialog: None,
            path_dialog_error: None,
            report_saved: None,
            report_clipboard_at: None,
            shortcut_status: None,
            diag_tx,
            diag_rx,
            diag_running: false,
            diag_summary: None,
            proxy_test_tx,
            proxy_test_rx,
            proxy_test_running: false,
            proxy_test_result: None,
            _tray_icon: tray_icon,
            tray_items,
            tray_hidden: start_hidden,
            autostart_enabled,
            latency_history: Vec::new(),
            last_rtt: None,
            current_tab: Tab::Main,
            last_diag_report: None,
            new_profile_name: String::new(),
            show_add_profile: false,
            last_ping_time: None,
            frames: 0,
        }
    }

    fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    /// Runs the manual update check on its own thread. The result lands in
    /// the footer; an update found also raises the banner, which is where the
    /// link lives.
    pub fn check_update_manually(&mut self, ctx: &egui::Context) {
        if self.manual_running {
            return;
        }
        self.manual_running = true;
        let tx = self.manual_tx.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("update-manual".to_string())
            .spawn(move || {
                let _ = tx.send(update::manual_check());
                ctx.request_repaint();
            })
            .ok();
    }

    pub fn start_diagnostics(&mut self, ctx: &egui::Context) {
        if self.diag_running {
            return;
        }
        self.diag_running = true;
        self.diag_summary = None;
        let tx = self.diag_tx.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("gui-diag".to_string())
            .spawn(move || {
                let report = crate::diag::run_diagnostics();
                let _ = tx.send(report);
                ctx.request_repaint();
            })
            .ok();
    }

    pub fn start_proxy_test(&mut self, ctx: &egui::Context) {
        if self.proxy_test_running {
            return;
        }
        let input = self.own_proxy_input.trim().to_string();
        if input.is_empty() {
            return;
        }
        self.proxy_test_running = true;
        self.proxy_test_result = None;
        let tx = self.proxy_test_tx.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("proxy-test".to_string())
            .spawn(move || {
                let start = std::time::Instant::now();
                let res = (|| -> Result<(u128, Option<String>), String> {
                    let first = input.split(';').next().unwrap_or(&input).trim();
                    let up = crate::upstream::parse(first)?;
                    let _sock = crate::upstream::open(
                        &up,
                        "daily-cloudcode-pa.googleapis.com",
                        443,
                        std::time::Duration::from_secs(6),
                    )?;
                    let rtt = start.elapsed().as_millis();
                    let loc = crate::upstream::exit_info(&up)
                        .map(|(ip, country)| format!("{ip} ({country})"));
                    Ok((rtt, loc))
                })();
                let _ = tx.send(res);
                ctx.request_repaint();
            })
            .ok();
    }

    /// The "новая версия" button, drawn on both screens (spec item 10: it must
    /// not disappear after the key is accepted).
    fn update_banner(&self, ui: &mut egui::Ui) {
        let Some(rel) = &self.update else { return };
        let label = format!("↑  Доступна новая версия — {}", rel.display_version());
        let btn = egui::Button::new(egui::RichText::new(label).color(egui::Color32::BLACK))
            .fill(theme::WARN)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS_SMALL))
            .min_size(egui::vec2(ui.available_width(), 32.0));
        if ui.add(btn).clicked() {
            // The feed's own link for this platform — or its page when no
            // archive matches — not a URL reconstructed from the version this
            // process happened to see hours ago.
            if let Some(url) = rel.landing_url() {
                crate::utils::open_url(url);
            }
        }
        ui.add_space(10.0);
    }

    /// Re-launches this exe elevated and closes the current window.
    ///
    /// The privileged half cannot be acquired in place — elevation is per
    /// process on Windows and eframe owns the message loop — so the only honest
    /// button is one that starts again. If the user dismisses the UAC prompt
    /// nothing happens and the window stays as it was.
    fn request_elevation(&self) {
        #[cfg(target_os = "windows")]
        {
            if crate::utils::relaunch_elevated() {
                std::process::exit(0);
            }
        }
    }

    fn drain_events(&mut self) {
        while let Ok(rel) = self.update_rx.try_recv() {
            if self.tray_hidden {
                crate::platform::notify::send(
                    "Open Antigravity",
                    &format!("Доступна новая версия: v{}", rel.display_version()),
                );
            }
            self.update = Some(rel);
        }
        while let Ok(res) = self.manual_rx.try_recv() {
            self.manual_running = false;
            let text = match res {
                Ok(Some(rel)) => {
                    self.update = Some(rel.clone());
                    format!(
                        "Доступна версия {} — ссылка в баннере сверху",
                        rel.display_version()
                    )
                }
                Ok(None) => format!(
                    "Обновлений нет — {} актуальна",
                    update::current_version()
                ),
                Err(e) => format!("Проверка не удалась: {e}"),
            };
            self.manual_result = Some((text, std::time::Instant::now()));
        }
        while let Ok(report) = self.diag_rx.try_recv() {
            self.diag_running = false;
            let status = report.overall_status();
            for item in &report.items {
                let level = match item.status {
                    crate::diag::DiagStatus::Ok => Level::Ok,
                    crate::diag::DiagStatus::Warn => Level::Warn,
                    crate::diag::DiagStatus::Fail => Level::Err,
                    crate::diag::DiagStatus::Info => Level::Info,
                };
                if self.log.len() >= LOG_LIMIT {
                    self.log.remove(0);
                }
                self.log.push((level, format!("{}: {}", item.name, item.summary)));
            }
            let verdict = match status {
                crate::diag::DiagStatus::Ok => "Тест сети: все проверки пройдены успешно (см. Журнал)",
                crate::diag::DiagStatus::Warn => "Тест сети: есть замечания — см. Журнал",
                crate::diag::DiagStatus::Fail => "Тест сети: обнаружены сбои — см. Журнал",
                crate::diag::DiagStatus::Info => "Тест сети завершён (см. Журнал)",
            };
            if let Some(ms) = report.latency_ms {
                self.last_rtt = Some(ms);
                self.latency_history.push(ms);
                if self.latency_history.len() > 30 {
                    self.latency_history.remove(0);
                }
            }
            if self.tray_hidden {
                crate::platform::notify::send("Open Antigravity: Тест сети", verdict);
            }
            self.diag_summary = Some((std::time::Instant::now(), status, verdict.to_string()));
            self.last_diag_report = Some(report);
        }
        // The footer message is a receipt, not a fixture: it clears itself.
        if let Some((_, at)) = &self.manual_result {
            if at.elapsed() > std::time::Duration::from_secs(15) {
                self.manual_result = None;
            }
        }
        while let Ok(res) = self.proxy_test_rx.try_recv() {
            self.proxy_test_running = false;
            if let Ok((rtt, _)) = &res {
                let ms = *rtt as u32;
                self.last_rtt = Some(ms);
                self.latency_history.push(ms);
                if self.latency_history.len() > 30 {
                    self.latency_history.remove(0);
                }
            }
            self.proxy_test_result = Some((res, std::time::Instant::now()));
        }
        if let Some((_, at)) = &self.proxy_test_result {
            if at.elapsed() > std::time::Duration::from_secs(30) {
                self.proxy_test_result = None;
            }
        }
        while let Ok(signal) = self.gate_rx.try_recv() {
            match signal {
                gate::Signal::Gate(view) => {
                    self.gate = *view;
                    self.gate_at = std::time::Instant::now();
                    if let Some(ms) = self.gate.relay.as_ref().and_then(|r| r.routes.first()).and_then(|row| row.latency_ms) {
                        self.last_rtt = Some(ms);
                        self.latency_history.push(ms);
                        if self.latency_history.len() > 30 {
                            self.latency_history.remove(0);
                        }
                    }
                }
                // The watcher decides *when* it is worth re-measuring; the
                // worker is the only thing that may take the measurement, so the
                // request passes through here rather than going around it.
                gate::Signal::MeasureVpn => self.worker.send(Cmd::RemeasureVpn),
            }
        }
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Log(level, line) => {
                    if self.log.len() >= LOG_LIMIT {
                        self.log.remove(0);
                    }
                    if self.tray_hidden
                        && level == Level::Ok
                        && (line.contains("пропатчен")
                            || line.contains("обновлен")
                            || line.contains("восстановлен"))
                    {
                        crate::platform::notify::send("Open Antigravity", &line);
                    }
                    self.log.push((level, line));
                }
                Event::Status(status) => {
                    // The field is only refilled when the user is not mid-typing,
                    // otherwise a status push landing between keystrokes would
                    // overwrite what they are entering.
                    if self.own_proxy_input.is_empty() && !status.own_proxy_text.is_empty() {
                        self.own_proxy_input = status.own_proxy_text.clone();
                    }
                    // A snapshot that arrives mid-drag must not overwrite the
                    // order under the pointer; the worker's copy is adopted
                    // again as soon as the drag is over.
                    if !self.providers_reordering {
                        self.providers_local = status.providers.clone();
                    }
                    self.status = Some(*status);
                }
                Event::Busy(what) => self.busy = what,
            }
        }

        // Keep the tray menu's status label in sync with the worker.
        if let Some(items) = &self.tray_items {
            let label = if let Some(st) = &self.status {
                if st.client_patch.is_on() {
                    "Статус: разблокировано ✓"
                } else {
                    "Статус: обход выключен"
                }
            } else {
                "Статус: ожидание…"
            };
            tray::update_status_label(items, label);
        }
    }

    /// Process clicks on the tray icon and its context menu.
    fn drain_tray_events(&mut self, ctx: &egui::Context) {
        let items = match &self.tray_items {
            Some(i) => i,
            None => return,
        };
        match tray::drain(items) {
            tray::TrayAction::None => {}
            tray::TrayAction::Show => {
                self.tray_hidden = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            tray::TrayAction::Quit => {
                // Real exit — do not intercept this close.
                self.tray_hidden = false;
                self.tray_items = None; // disarm the close-intercept
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            tray::TrayAction::RunDiag => {
                self.start_diagnostics(ctx);
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();

        // --- System tray: hide-to-tray on window close ---
        // Process tray icon / menu clicks even when the window is hidden
        // (egui calls logic() on request_repaint while window is hidden).
        self.drain_tray_events(ctx);

        // When the user clicks the window's [X], let it close cleanly and exit.
        if ctx.input(|i| i.viewport().close_requested()) {
            self.tray_items = None;
        }

        // Live proxy ping check: runs automatically every 25 seconds if proxy is configured and enabled
        let proxy_on = self.status.as_ref().map(|s| s.get(crate::core::ops::Cap::OwnProxy).is_on()).unwrap_or(true);
        let need_ping = match self.last_ping_time {
            Some(t) => t.elapsed() > std::time::Duration::from_secs(25),
            None => true,
        };
        if need_ping && proxy_on && !self.proxy_test_running && !self.own_proxy_input.trim().is_empty() {
            self.last_ping_time = Some(std::time::Instant::now());
            self.start_proxy_test(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // By the third pass two frames have been presented, which is where a
        // renderer that was going to fail at the swap chain has failed. egui
        // would otherwise sleep after the first one, so it is asked for more.
        if self.frames < 3 {
            if self.frames == 1 {
                renderer::dev_break_frame();
            }
            self.frames += 1;
            ui.ctx().request_repaint();
        } else if self.frames == 3 {
            self.frames += 1;
            renderer::confirm();
        }

        // The root `Ui` eframe hands over carries no margin and no background of
        // its own; `central_panel` is what puts the window colour behind it.
        //
        // `set_min_size` is not cosmetic padding: a frame sized to its contents
        // paints only as far down as the contents reach, and the rest of the
        // window shows eframe's clear colour instead — a visible seam across the
        // window wherever the screen is shorter than it is.
        const MARGIN_X: f32 = 16.0;
        const MARGIN_TOP: f32 = 14.0;
        const MARGIN_BOTTOM: f32 = 18.0;
        let avail = ui.available_size();
        let frame = egui::Frame::central_panel(&ui.style().clone()).inner_margin(egui::Margin {
            left: MARGIN_X as i8,
            right: MARGIN_X as i8,
            top: MARGIN_TOP as i8,
            bottom: MARGIN_BOTTOM as i8,
        });
        frame.show(ui, |ui| {
            ui.set_min_height(avail.y - MARGIN_TOP - MARGIN_BOTTOM);
            main_view::view(self, ui);
        });

        let ctx = ui.ctx().clone();
        main_view::path_dialog(self, &ctx);
    }
}

/// Opens the window. Returns only when the user closes it.
///
/// Renderers are tried down `renderer::CHAIN` — on Windows DirectX 12, then
/// DirectX 12 on the software rasteriser, then OpenGL — starting from whatever
/// the last start on this machine learned (`renderer::first`). An error moves
/// to the next one in this process; a panic or a crash inside a driver moves to
/// it on the next start (see `renderer`). Falling back costs nothing at runtime
/// and is the difference between "it starts on other PCs" and a support thread.
pub fn run() -> Result<(), String> {
    fn options(kind: renderer::Kind) -> eframe::NativeOptions {
        let start_hidden = std::env::args().any(|a| a == "--minimized" || a == "-m" || a == "--tray");
        eframe::NativeOptions {
            viewport: {
                let mut vp = egui::ViewportBuilder::default()
                    .with_inner_size([WIN_W, WIN_H])
                    .with_min_inner_size([480.0, 560.0])
                    .with_visible(!start_hidden)
                    .with_title(title());
                // The exe resource covers Explorer and the taskbar; this is what
                // puts the same picture in the title bar and Alt-Tab.
                if let Some(ico) = icon::window_icon() {
                    vp = vp.with_icon(ico);
                }
                vp
            },
            renderer: kind.renderer(),
            wgpu_options: kind.wgpu_options(),
            ..Default::default()
        }
    }

    renderer::install();
    let mut failures: Vec<String> = Vec::new();
    let mut kind = Some(renderer::first());
    while let Some(k) = kind {
        renderer::attempting(k);
        match eframe::run_native(
            &title(),
            options(k),
            Box::new(|cc| Ok(Box::new(App::new(cc)))),
        ) {
            Ok(()) => return Ok(()),
            // No display to open a window on at all: no X server or Wayland
            // compositor, or one this user may not use (`sudo` — "Authorization
            // required"). Not a renderer's failure, so the next one would fail
            // the same way, and winit would refuse it anyway: an event loop
            // cannot be created twice in one process ("EventLoop can't be
            // recreated", which is all the second attempt used to report).
            Err(e @ eframe::Error::WinitEventLoop(_)) => {
                renderer::forget();
                return Err(format!(
                    "нет доступа к графическому дисплею: {}. \
                     Из терминала (Konsole) программа работает в текстовом режиме: \
                     bash launch.sh. Если это статическая musl-сборка, окно ей \
                     недоступно в принципе — нужен glibc-билд (BUILD_PORTABLE.md, п. 8).",
                    without_source_location(&e.to_string())
                ));
            }
            Err(e) => failures.push(format!("{}: {e}", k.label())),
        }
        kind = k.next();
    }
    // Nothing opened, and every renderer said why rather than crashing: that
    // says more about this start (a session with no desktop) than about the
    // machine, so the next start gets the whole chain again.
    renderer::forget();
    Err(format!("не удалось открыть окно ({})", failures.join("; ")))
}

/// winit's `os error at <file>.rs:<line>: <what>` is for its own developers;
/// the user gets `<what>`.
fn without_source_location(msg: &str) -> &str {
    msg.find(".rs:")
        .and_then(|i| msg[i..].find(": ").map(|j| &msg[i + j + 2..]))
        .unwrap_or(msg)
}

fn title() -> String {
    format!("Open Antigravity v{}", update::current_version())
}

#[cfg(test)]
mod tests {
    use super::without_source_location;

    #[test]
    fn a_winit_error_loses_the_build_machine_path_and_keeps_what_happened() {
        assert_eq!(
            without_source_location(
                "os error at /cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-0.30.13/src/platform_impl/linux/mod.rs:788: Failed to open connection to X server"
            ),
            "Failed to open connection to X server"
        );
        assert_eq!(without_source_location("no display"), "no display");
    }
}
