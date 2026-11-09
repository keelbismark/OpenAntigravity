//! The terminal front end: the same switches as the window, for a machine that
//! cannot or should not open one — a server over SSH, a desktop whose graphics
//! driver draws nothing, a user who would rather type.
//!
//! Nothing here decides anything. The verdict on top is `gui::status` (the pure
//! function the window's card calls), every action is an `ops` command, the
//! report is `gui::report`. Only the drawing and the keys belong to this file,
//! so the two front ends cannot drift into saying different things.
//!
//! Where it runs (`main`): `--tui` anywhere; on Linux also any start with no
//! graphical session but a terminal (a server over SSH); on Windows also when no
//! renderer could open the window at all. On Windows the exe is a GUI-subsystem
//! program, which cmd and PowerShell do not wait for — sharing their console
//! would split every keystroke between the shell's prompt and this screen — so
//! it gets a console of its own, except over SSH where there is no desktop to
//! put one on (see `console`).

use std::io::Write as _;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    BorderType, ListItem, ListState,
};
use ratatui::{DefaultTerminal, Frame};

use crate::gate;
use crate::gui::status::{self, Action, Facts, Tone};
use crate::ops::{self, Cap, Cmd, Event, Level, State, Status, Worker};
use crate::update::{self, UpdateInfo};

/// How many log lines are kept, as in the window.
const LOG_LIMIT: usize = 400;
/// Below this the layout cannot hold the card, the list and the journal.
const MIN_W: u16 = 50;
const MIN_H: u16 = 18;

/// `--tui` (or `tui`, any number of dashes) anywhere on the command line.
pub fn requested() -> bool {
    std::env::args()
        .skip(1)
        .any(|a| a.trim_start_matches('-').eq_ignore_ascii_case("tui"))
}

/// Linux: a terminal and nothing to draw a window into — a server over SSH.
#[cfg(not(windows))]
pub fn only_terminal() -> bool {
    use std::io::IsTerminal;
    std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
        && std::io::stdin().is_terminal()
}

/// Runs until the user quits. `note` is shown first in the journal — why the
/// window did not open, when that is why we are here.
pub fn run(note: Option<String>) -> Result<(), String> {
    #[cfg(windows)]
    {
        console::open()?;
        // A console this process opened closes with it, and the panic text with
        // it. Replaces the window's hook too, if the window was tried first: its
        // relaunch-on-the-next-renderer has nothing left to do here. ratatui's
        // own hook (below) puts the terminal back before this runs.
        let _ = std::panic::take_hook();
        std::panic::set_hook(Box::new(|info| {
            crate::utils::message_box(
                "Open Antigravity",
                &format!("Программа аварийно завершилась.\n\n{info}"),
            );
        }));
    }

    let mut terminal = ratatui::try_init().map_err(|e| format!("терминал недоступен: {e}"))?;
    // Bracketed paste makes a pasted key one event instead of 25 keystrokes.
    // Windows' console has no such mode; there a paste arrives as keys, which
    // the key field takes just as well.
    #[cfg(not(windows))]
    let _ = ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::EnableBracketedPaste
    );

    let result = App::new(note).run(&mut terminal);

    #[cfg(not(windows))]
    let _ = ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::DisableBracketedPaste
    );
    ratatui::restore();
    result
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A line being typed into, drawn as a box over the list.
enum Input {
    Path { text: String, error: Option<String> },
    OwnProxy { text: String },
}

/// One selectable line of the main list.
#[derive(Clone, Copy, PartialEq)]
enum Row {
    /// The status card's one action, when it has one.
    Action(Action),
    Cap(Cap),
    /// The bypass master switch (`ops::bypass_order`).
    Bypass,
    Install(usize),
    /// The fold over «Настройки для опытных».
    Advanced,
    Provider(usize),
    OwnProxyText,
    Report,
}

struct App {
    update: Option<UpdateInfo>,
    update_rx: Receiver<UpdateInfo>,

    /// The manual check ('u'): result lands in the toast, an update found
    /// also raises the title banner.
    manual_tx: std::sync::mpsc::Sender<Result<Option<UpdateInfo>, String>>,
    manual_rx: Receiver<Result<Option<UpdateInfo>, String>>,
    manual_running: bool,
    auto_update_tx: std::sync::mpsc::Sender<crate::update::UpdateProgress>,
    auto_update_rx: Receiver<crate::update::UpdateProgress>,
    auto_update_running: bool,

    diag_tx: std::sync::mpsc::Sender<crate::diag::DiagReport>,
    diag_rx: Receiver<crate::diag::DiagReport>,
    diag_running: bool,

    worker: Worker,
    events: Receiver<Event>,
    status: Option<Status>,
    gate: gate::View,
    gate_at: Instant,
    gate_rx: Receiver<gate::Signal>,
    log: Vec<(Level, String)>,
    busy: Option<String>,

    list: ListState,
    /// The row the cursor is on, by what it is rather than where: rows come and
    /// go under it (the card's action appears and disappears, installs are
    /// found), and an index would then point a keystroke at the row that slid
    /// into its place - a different switch than the one on screen.
    selected: Row,
    advanced: bool,
    input: Option<Input>,
    /// A line in place of the key hints for a few seconds: what just happened.
    toast: Option<(String, Instant)>,
    quit: bool,
}

impl App {
    fn new(note: Option<String>) -> Self {
        let (up_tx, update_rx) = channel();
        update::spawn_watch(up_tx, Box::new(|| {}));
        let (manual_tx, manual_rx) = channel();
        let (diag_tx, diag_rx) = channel();
        let (auto_update_tx, auto_update_rx) = channel();
        // The loop below polls every 200 ms, so nothing needs waking.
        let (ev_tx, events) = channel();
        let worker = ops::spawn(ev_tx, Box::new(|| {}));
        let (gate_tx, gate_rx) = channel();
        gate::spawn_watch(gate_tx, Box::new(|| {}));

        let mut log = Vec::new();
        if let Some(note) = note {
            log.push((Level::Warn, note));
        }
        // `sudo ./open_antigravity`: everything here is per user (the installs under
        // ~/.local/share, the systemd user unit, environment.d), and under sudo
        // "the user" is root. A server where root is the user sets no SUDO_USER.
        #[cfg(not(windows))]
        if let Some(user) = std::env::var("SUDO_USER").ok().filter(|u| !u.is_empty()) {
            if crate::utils::is_admin() {
                log.push((
                    Level::Warn,
                    format!(
                        "Запущено через sudo: Antigravity ищется в домашней папке root, а не {user}. \
                         Запустите без sudo — права root не нужны."
                    ),
                ));
            }
        }
        worker.send(Cmd::Unlocked);
        let mut list = ListState::default();
        list.select(Some(0));
        Self {
            update: None,
            update_rx,
            manual_tx,
            manual_rx,
            manual_running: false,
            auto_update_tx,
            auto_update_rx,
            auto_update_running: false,
            diag_tx,
            diag_rx,
            diag_running: false,
            worker,
            events,
            status: None,
            gate: gate::View::default(),
            gate_at: Instant::now(),
            gate_rx,
            log,
            busy: None,
            list,
            selected: Row::Cap(Cap::ClientPatch),
            advanced: false,
            input: None,
            toast: None,
            quit: false,
        }
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> Result<(), String> {
        let mut drawn_at: Option<Instant> = None;
        let mut dirty = true;
        while !self.quit {
            dirty |= self.drain();
            // Ages on the card count up by themselves, so a quiet screen is
            // still redrawn once a second.
            if dirty || drawn_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
                terminal
                    .draw(|f| self.draw(f))
                    .map_err(|e| format!("терминал: {e}"))?;
                drawn_at = Some(Instant::now());
                dirty = false;
            }
            if event::poll(Duration::from_millis(200)).map_err(|e| format!("терминал: {e}"))?
            {
                match event::read().map_err(|e| format!("терминал: {e}"))? {
                    // Windows reports releases too; only a press is a keystroke.
                    TermEvent::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
                    TermEvent::Paste(text) => self.on_paste(&text),
                    _ => {}
                }
                dirty = true;
            }
        }
        Ok(())
    }

    /// The manual update check, on its own thread like everywhere else: the
    /// UI loop must not stall on a feed that answers slowly or not at all.
    fn start_auto_update_now(&mut self) {
        if self.auto_update_running {
            self.toast("Обновление уже выполняется…");
            return;
        }
        let Some(rel) = self.update.clone() else {
            self.toast("Сначала проверьте наличие обновлений (клавиша 'u')");
            return;
        };
        self.auto_update_running = true;
        self.toast(format!("Запуск автообновления до v{}…", rel.display_version()));
        let tx = self.auto_update_tx.clone();
        std::thread::Builder::new()
            .name("tui-auto-update".to_string())
            .spawn(move || {
                crate::update::perform_auto_update(&rel, tx, std::sync::Arc::new(|| {}));
            })
            .ok();
    }

    fn check_updates_now(&mut self) {
        if self.manual_running {
            self.toast("Проверка уже идёт…");
            return;
        }
        if update::update_url().is_none() {
            self.toast("URL обновлений не задан (OAG_UPDATE_URL)");
            return;
        }
        self.manual_running = true;
        self.toast("Проверяю обновления…");
        let tx = self.manual_tx.clone();
        std::thread::Builder::new()
            .name("update-manual".to_string())
            .spawn(move || {
                let _ = tx.send(update::manual_check());
            })
            .ok();
    }

    fn start_diagnostics_now(&mut self) {
        if self.diag_running {
            self.toast("Диагностика уже выполняется…");
            return;
        }
        self.diag_running = true;
        self.toast("Запуск комплексного теста сети…");
        if self.log.len() >= LOG_LIMIT {
            self.log.remove(0);
        }
        self.log.push((Level::Step, "Запуск самодиагностики сети и системы...".into()));
        let tx = self.diag_tx.clone();
        std::thread::Builder::new()
            .name("tui-diag".into())
            .spawn(move || {
                let report = crate::diag::run_diagnostics();
                let _ = tx.send(report);
            })
            .ok();
    }

    fn create_shortcuts_now(&mut self) {
        match crate::shortcut::create_shortcuts() {
            Ok(msg) => {
                self.toast(msg.clone());
                if self.log.len() >= LOG_LIMIT {
                    self.log.remove(0);
                }
                self.log.push((Level::Ok, msg));
            }
            Err(err) => {
                self.toast(format!("Ошибка: {err}"));
                if self.log.len() >= LOG_LIMIT {
                    self.log.remove(0);
                }
                self.log.push((Level::Err, format!("Создание ярлыка: {err}")));
            }
        }
    }

    fn toggle_autostart_now(&mut self) {
        let current = crate::platform::autostart::is_enabled();
        let target = !current;
        match crate::platform::autostart::set_enabled(target) {
            Ok(()) => {
                let msg = if target {
                    "Автозапуск с системой включён".to_string()
                } else {
                    "Автозапуск с системой выключен".to_string()
                };
                self.toast(msg.clone());
                if self.log.len() >= LOG_LIMIT {
                    self.log.remove(0);
                }
                self.log.push((Level::Ok, msg));
            }
            Err(err) => {
                let msg = format!("Ошибка автозапуска: {err}");
                self.toast(msg.clone());
                if self.log.len() >= LOG_LIMIT {
                    self.log.remove(0);
                }
                self.log.push((Level::Err, msg));
            }
        }
    }

    /// Takes in everything the worker and the watchers sent. True when anything
    /// arrived.
    fn drain(&mut self) -> bool {
        let mut any = false;
        while let Ok(prog) = self.auto_update_rx.try_recv() {
            any = true;
            match prog {
                crate::update::UpdateProgress::Downloading { percent, text } => {
                    self.toast(format!("Загрузка обновления: {percent}% ({text})"));
                }
                crate::update::UpdateProgress::Verifying => {
                    self.toast("Проверка контрольной суммы (SHA-256)…");
                }
                crate::update::UpdateProgress::Extracting => {
                    self.toast("Распаковка и установка файлов…");
                }
                crate::update::UpdateProgress::Restarting => {
                    self.toast("Обновление установлено! Перезапуск…");
                }
                crate::update::UpdateProgress::Failed(err) => {
                    self.auto_update_running = false;
                    self.toast(format!("Ошибка обновления: {err}"));
                }
            }
        }
        while let Ok(rel) = self.update_rx.try_recv() {
            self.update = Some(rel);
            any = true;
        }
        while let Ok(res) = self.manual_rx.try_recv() {
            self.manual_running = false;
            any = true;
            match res {
                Ok(Some(rel)) => {
                    self.toast(format!(
                        "Доступна версия {} — ссылка в заголовке",
                        rel.display_version()
                    ));
                    self.update = Some(rel);
                }
                Ok(None) => self.toast(format!(
                    "Обновлений нет — {} актуальна",
                    update::current_version()
                )),
                Err(e) => self.toast(format!("Проверка не удалась: {e}")),
            }
        }
        while let Ok(report) = self.diag_rx.try_recv() {
            self.diag_running = false;
            any = true;
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
                crate::diag::DiagStatus::Ok => "Тест сети: всё в норме, связь и прокси работают",
                crate::diag::DiagStatus::Warn => "Тест сети: есть замечания — см. журнал",
                crate::diag::DiagStatus::Fail => "Тест сети: обнаружены сбои — см. журнал",
                crate::diag::DiagStatus::Info => "Тест сети завершён",
            };
            self.toast(verdict);
        }
        while let Ok(signal) = self.gate_rx.try_recv() {
            match signal {
                gate::Signal::Gate(view) => {
                    self.gate = *view;
                    self.gate_at = Instant::now();
                }
                // The worker owns that measurement (I59), as in the window.
                gate::Signal::MeasureVpn => self.worker.send(Cmd::RemeasureVpn),
            }
            any = true;
        }
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Log(level, line) => {
                    if self.log.len() >= LOG_LIMIT {
                        self.log.remove(0);
                    }
                    self.log.push((level, line));
                }
                Event::Status(s) => self.status = Some(*s),
                Event::Busy(what) => self.busy = what,
            }
            any = true;
        }
        any
    }

    fn facts(&self) -> Option<Facts> {
        let s = self.status.as_ref()?;
        Some(Facts::read(s, &self.gate, self.gate_at.elapsed()))
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    // -----------------------------------------------------------------------
    // Keys
    // -----------------------------------------------------------------------

    fn on_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // Raw mode turns Ctrl+C into a key; it still means "leave".
        if ctrl && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.quit = true;
            return;
        }
        if self.input.is_some() {
            self.on_input_key(k, ctrl);
        } else {
            self.on_main_key(k);
        }
    }

    fn on_paste(&mut self, text: &str) {
        let text = text.trim();
        if let Some(Input::Path { text: t, .. } | Input::OwnProxy { text: t }) =
            &mut self.input
        {
            t.push_str(text);
        }
    }

    fn on_input_key(&mut self, k: KeyEvent, ctrl: bool) {
        let Some(input) = &mut self.input else { return };
        let text = match input {
            Input::Path { text, .. } | Input::OwnProxy { text } => text,
        };
        match k.code {
            KeyCode::Esc => self.input = None,
            KeyCode::Enter => self.submit_input(),
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char('u') if ctrl => text.clear(),
            KeyCode::Char(c) if !ctrl => text.push(c),
            _ => {}
        }
    }

    fn submit_input(&mut self) {
        match self.input.take() {
            Some(Input::Path { text, .. }) => {
                let cleaned = crate::clean_input_path(&text);
                if cleaned.is_empty() {
                    return;
                }
                match ops::resolve_manual_path(std::path::Path::new(&cleaned)) {
                    Some(root) => self.worker.send(Cmd::AddPath(root)),
                    None => {
                        self.input = Some(Input::Path {
                            text,
                            error: Some("По этому пути установка Antigravity не найдена.".into()),
                        })
                    }
                }
            }
            Some(Input::OwnProxy { text }) => {
                self.worker.send(Cmd::SetOwnProxy(text.trim().to_string()))
            }
            None => {}
        }
    }

    /// Where the cursor's row is now, and whether it is still there at all. When
    /// it is gone the cursor stays at the same height, on whatever row is there.
    fn cursor(&mut self, rows: &[Row]) -> (usize, bool) {
        let last = rows.len().saturating_sub(1);
        match rows.iter().position(|r| *r == self.selected) {
            Some(at) => (at, true),
            None => {
                let at = self.list.selected().unwrap_or(0).min(last);
                if let Some(r) = rows.get(at) {
                    self.selected = *r;
                }
                (at, false)
            }
        }
    }

    fn go(&mut self, rows: &[Row], at: usize) {
        if let Some(r) = rows.get(at) {
            self.selected = *r;
            self.list.select(Some(at));
        }
    }

    fn on_main_key(&mut self, k: KeyEvent) {
        let rows = self.rows();
        let last = rows.len().saturating_sub(1);
        let (at, still_there) = self.cursor(&rows);
        let row = rows.get(at).copied();
        // The row that was on screen under the cursor has just gone: a key that
        // acts on a row was aimed at it, not at its replacement.
        let acts_on_row = matches!(
            k.code,
            KeyCode::Enter
                | KeyCode::Char(' ')
                | KeyCode::Char('x')
                | KeyCode::Delete
                | KeyCode::Char('+')
                | KeyCode::Char('-')
        );
        if acts_on_row && !still_there {
            self.go(&rows, at);
            return;
        }
        match k.code {
            KeyCode::Char('q') => {
                if self.auto_update_running {
                    self.toast("Идёт установка обновления, пожалуйста, подождите…");
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.go(&rows, at.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.go(&rows, (at + 1).min(last)),
            KeyCode::PageUp => self.go(&rows, at.saturating_sub(8)),
            KeyCode::PageDown => self.go(&rows, (at + 8).min(last)),
            KeyCode::Home => self.go(&rows, 0),
            KeyCode::End => self.go(&rows, last),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(row) = row {
                    self.activate(row);
                }
            }
            // Only what the card offers right now, as in the window: «Включить
            // всё» closes Antigravity, and neither belongs to a stray keystroke
            // when the card says there is nothing to do.
            KeyCode::Char('a') => self.act_if_offered(Action::EnableAll),
            KeyCode::Char('r') => self.act_if_offered(Action::Repair),
            KeyCode::Char('e') => self.act_if_offered(Action::KillHolder),
            KeyCode::Char('v') => self.act_if_offered(Action::Verify),
            KeyCode::Char('d') => self.start_diagnostics_now(),
            KeyCode::Char('s') => self.create_shortcuts_now(),
            KeyCode::Char('t') => self.toggle_autostart_now(),
            KeyCode::Char('o') => self.save_report(),
            KeyCode::Char('u') => self.check_updates_now(),
            KeyCode::Char('U') => self.start_auto_update_now(),
            KeyCode::Char('x') | KeyCode::Delete => {
                if let Some(Row::Install(i)) = row {
                    self.forget_install(i);
                }
            }
            KeyCode::Char('+') | KeyCode::Char('-') => {
                if let Some(Row::Provider(i)) = row {
                    self.move_provider(i, k.code == KeyCode::Char('-'));
                }
            }
            _ => {}
        }
    }

    fn busy_refusal(&mut self) -> bool {
        if let Some(what) = self.busy.clone() {
            self.toast(format!("Подождите — идёт: {what}"));
            return true;
        }
        false
    }

    fn activate(&mut self, row: Row) {
        match row {
            Row::Action(a) => self.act(a),
            Row::Cap(cap) => {
                if self.busy_refusal() {
                    return;
                }
                let state = self
                    .status
                    .as_ref()
                    .map(|s| s.get(cap).clone())
                    .unwrap_or(State::Off);
                if let State::Blocked(why) = &state {
                    self.toast(format!("Не включается: {why}"));
                    return;
                }
                if cap == Cap::OwnProxy && !state.is_on() && self.own_proxy_text().is_empty() {
                    self.input = Some(Input::OwnProxy {
                        text: String::new(),
                    });
                    return;
                }
                self.worker.send(Cmd::Set(cap, !state.is_on()));
            }
            Row::Bypass => {
                if self.busy_refusal() {
                    return;
                }
                let on = !self.status.as_ref().is_some_and(|s| s.bypass_on());
                for cap in ops::bypass_order(on) {
                    self.worker.send(Cmd::Set(cap, on));
                }
            }
            Row::Install(i) => {
                let text = self.install_path(i).unwrap_or_default();
                self.input = Some(Input::Path { text, error: None });
            }
            Row::Advanced => self.advanced = !self.advanced,
            Row::Provider(i) => {
                if self.busy_refusal() {
                    return;
                }
                if let Some(p) = self.status.as_ref().and_then(|s| s.providers.get(i)) {
                    self.worker
                        .send(Cmd::SetProvider(p.name.clone(), !p.enabled));
                }
            }
            Row::OwnProxyText => {
                self.input = Some(Input::OwnProxy {
                    text: self.own_proxy_text(),
                })
            }
            Row::Report => self.save_report(),
        }
    }

    fn act_if_offered(&mut self, action: Action) {
        let offered = self.facts().and_then(|f| status::headline(&f).action);
        match offered {
            Some(a) if a == action => self.act(action),
            Some(a) => self.toast(format!("Сейчас нужно другое: {}.", a.label())),
            None => self.toast(format!("«{}» сейчас не нужно.", action.label())),
        }
    }

    fn act(&mut self, action: Action) {
        if self.busy_refusal() {
            return;
        }
        match action {
            Action::EnableAll => self.worker.send(Cmd::EnableAll),
            Action::Repair => self.worker.send(Cmd::Repair),
            Action::KillHolder => self.worker.send(Cmd::KillHolder),
            Action::Elevate => {
                #[cfg(windows)]
                if crate::utils::relaunch_elevated_with("--tui") {
                    self.quit = true;
                    return;
                }
                self.toast("Права администратора не получены.");
            }
            // Opened, and also shown: a terminal with no desktop behind it has
            // no browser to open, and the link is then copied from here.
            Action::Verify => {
                if let Some((_, url)) = self.facts().and_then(|f| f.verify) {
                    crate::utils::open_url_as_user(&url);
                    self.toast(format!("Если браузер не открылся, откройте ссылку вручную: {url}"));
                }
            }
        }
    }

    fn install_path(&self, i: usize) -> Option<String> {
        let row = self.status.as_ref()?.installs.get(i)?;
        row.path.as_ref().map(|p| p.display().to_string())
    }

    fn forget_install(&mut self, i: usize) {
        let Some(row) = self.status.as_ref().and_then(|s| s.installs.get(i)) else {
            return;
        };
        match (&row.path, row.manual) {
            (Some(p), true) => {
                let p = p.clone();
                self.worker.send(Cmd::ForgetPath(p));
            }
            _ => self.toast("Убрать можно только путь, указанный вручную."),
        }
    }

    fn move_provider(&mut self, i: usize, up: bool) {
        let Some(s) = self.status.as_ref() else {
            return;
        };
        let mut order: Vec<String> = s.providers.iter().map(|p| p.name.clone()).collect();
        let j = if up {
            i.checked_sub(1)
        } else {
            Some(i + 1).filter(|j| *j < order.len())
        };
        let Some(j) = j else { return };
        order.swap(i, j);
        // Moved locally too, so the selection can follow the row before the
        // worker's snapshot comes back with the same order.
        if let Some(s) = self.status.as_mut() {
            s.providers.swap(i, j);
        }
        self.selected = Row::Provider(j);
        self.worker.send(Cmd::ReorderProviders(order));
    }

    fn own_proxy_text(&self) -> String {
        self.status
            .as_ref()
            .map(|s| s.own_proxy_text.clone())
            .unwrap_or_default()
    }

    /// The window's «Сохранить отчёт». The file is the deliverable - a report
    /// pasted into a chat is a wall of text that arrives truncated - so it goes
    /// to the report dir (the portable data dir next to the exe, otherwise the
    /// Desktop), and to the state directory when there is no Desktop to write
    /// to (a server). It is also offered to the terminal (OSC 52: Windows
    /// Terminal, iTerm2, kitty, WezTerm, tmux pass it on to the clipboard of the
    /// machine the user sits at), which is the only clipboard an SSH session has.
    fn save_report(&mut self) {
        let text = crate::gui::report::build(self.status.as_ref(), &self.gate);
        let saved = crate::utils::report_dir()
            .and_then(|dir| crate::utils::save_text_file(&dir, crate::utils::REPORT_FILE, &text))
            .or_else(|| {
                let dir = crate::dns_forwarder::log_dir();
                std::fs::create_dir_all(&dir).ok();
                crate::utils::save_text_file(&dir, crate::utils::REPORT_FILE, &text)
            });
        let copied = copy_out(&text);
        let msg = match (saved, copied) {
            (Some(path), Copied::Clipboard) => {
                format!("Отчёт сохранён: {} (и скопирован)", path.display())
            }
            (Some(path), Copied::Terminal) => format!(
                "Отчёт сохранён: {} (и передан терминалу для буфера обмена)",
                path.display()
            ),
            (None, Copied::Clipboard) => {
                "Файл сохранить не удалось — отчёт скопирован в буфер обмена.".to_string()
            }
            (None, Copied::Terminal) => {
                "Файл сохранить не удалось — отчёт передан терминалу для буфера обмена.".to_string()
            }
        };
        self.toast(msg);
    }

    // -----------------------------------------------------------------------
    // Rows
    // -----------------------------------------------------------------------

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Some(a) = self.facts().and_then(|f| status::headline(&f).action) {
            rows.push(Row::Action(a));
        }
        rows.push(Row::Cap(Cap::ClientPatch));
        rows.push(Row::Bypass);
        rows.push(Row::Cap(Cap::Watchdog));
        let installs = self.status.as_ref().map_or(0, |s| s.installs.len());
        rows.extend((0..installs).map(Row::Install));
        rows.push(Row::Advanced);
        if self.advanced {
            rows.push(Row::Cap(Cap::Dns));
            let providers = self.status.as_ref().map_or(0, |s| s.providers.len());
            rows.extend((0..providers).map(Row::Provider));
            if providers > 0 {
                rows.push(Row::Cap(Cap::DnsRotation));
            }
            rows.push(Row::Cap(Cap::LocalProxy));
            rows.push(Row::Cap(Cap::BuiltinExits));
            rows.push(Row::Cap(Cap::VerifyTls));
            rows.push(Row::Cap(Cap::OwnProxy));
            rows.push(Row::OwnProxyText);
        }
        rows.push(Row::Report);
        rows
    }

    /// What the selected row does, for the line under the list.
    fn row_hint(&self, row: Row) -> String {
        match row {
            Row::Action(a) => format!("Enter — {}.", a.label()),
            Row::Cap(cap) => status::switch_text(cap).1.to_string(),
            Row::Bypass => status::BYPASS_TEXT.1.to_string(),
            Row::Install(i) => {
                let manual = self
                    .status
                    .as_ref()
                    .and_then(|s| s.installs.get(i))
                    .is_some_and(|r| r.manual);
                let full = self.install_path(i).unwrap_or_default();
                let mut s = "Enter — указать путь вручную".to_string();
                if manual {
                    s.push_str(", x — убрать указанный путь");
                }
                if !full.is_empty() {
                    s.push_str(&format!(". {full}"));
                }
                s
            }
            Row::Advanced => "Детали обхода: DNS-серверы, прокси, выходы.".into(),
            Row::Provider(_) => "Пробел — включить или выключить сервер, +/- — выше или ниже в списке.".into(),
            Row::OwnProxyText => {
                "Enter — изменить адрес: логин:пароль@адрес:порт (или адрес:порт без пароля).".into()
            }
            Row::Report => {
                "Всё, что нужно, чтобы понять, почему Antigravity отвечает или нет — одним текстом для группы.".into()
            }
        }
    }

    fn row_item(&self, row: Row, width: u16) -> ListItem<'static> {
        let busy = self.busy.is_some();
        match row {
            Row::Action(a) => ListItem::new(Line::from(vec![
                Span::styled("▶ ", Style::new().fg(Color::Cyan)),
                Span::styled(
                    a.label().to_string(),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
            ])),
            Row::Cap(cap) => {
                let state = self
                    .status
                    .as_ref()
                    .map(|s| s.get(cap).clone())
                    .unwrap_or(State::Off);
                let indent = if matches!(cap, Cap::ClientPatch | Cap::Watchdog) {
                    ""
                } else {
                    "  "
                };
                switch_line(indent, status::switch_text(cap).0, &state, busy, width)
            }
            Row::Bypass => {
                let on = self.status.as_ref().is_some_and(|s| s.bypass_on());
                let state = if on { State::On } else { State::Off };
                switch_line("", status::BYPASS_TEXT.0, &state, busy, width)
            }
            Row::Install(i) => {
                let Some(r) = self.status.as_ref().and_then(|s| s.installs.get(i)) else {
                    return ListItem::new("");
                };
                let (dot, color, what) = match (&r.path, r.patched) {
                    (None, _) => (
                        "○",
                        Color::DarkGray,
                        "не найдено — Enter: указать путь".to_string(),
                    ),
                    (Some(p), patched) => (
                        "●",
                        match patched {
                            Some(true) => Color::Green,
                            Some(false) => Color::Gray,
                            None => Color::DarkGray,
                        },
                        crate::utils::mask_path(&p.display().to_string()),
                    ),
                };
                let tail = match (&r.path, r.patched) {
                    (Some(_), Some(true)) => " · пропатчен",
                    (Some(_), Some(false)) => " · не пропатчен",
                    (Some(_), None) => " · проверяю…",
                    (None, _) => "",
                };
                ListItem::new(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(format!("{dot} "), Style::new().fg(color)),
                    Span::raw(format!("{}  ", r.label)),
                    Span::styled(what, Style::new().fg(Color::DarkGray)),
                    Span::styled(tail, Style::new().fg(color)),
                ]))
            }
            Row::Advanced => ListItem::new(Line::styled(
                format!(
                    "{} Настройки для опытных",
                    if self.advanced { "▾" } else { "▸" }
                ),
                Style::new().fg(Color::Gray),
            )),
            Row::Provider(i) => {
                let Some(p) = self.status.as_ref().and_then(|s| s.providers.get(i)) else {
                    return ListItem::new("");
                };
                let state = if p.enabled { State::On } else { State::Off };
                let name = format!("{}. {}", i + 1, status::provider_name(&p.name));
                switch_line("      ", &name, &state, busy, width)
            }
            Row::OwnProxyText => {
                let text = self.own_proxy_text();
                let shown = if text.is_empty() {
                    "не задан".to_string()
                } else {
                    text
                };
                ListItem::new(Line::from(vec![
                    Span::raw("      Адрес прокси: "),
                    Span::styled(shown, Style::new().fg(Color::Gray)),
                ]))
            }
            Row::Report => ListItem::new(Line::from(vec![
                Span::styled("⎘ ", Style::new().fg(Color::Cyan)),
                Span::raw("Сохранить отчёт"),
            ])),
        }
    }
}

mod render;
#[cfg(windows)]
mod console;
#[cfg(test)]
mod tests;

use render::*;
