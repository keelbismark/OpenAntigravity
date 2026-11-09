//! Rendering and UI formatting for the Terminal Control Center.

use super::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

impl App {
    pub(super) fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        if area.width < MIN_W || area.height < MIN_H {
            f.render_widget(
                Paragraph::new(format!(
                    "Окно терминала слишком маленькое: нужно хотя бы {MIN_W}×{MIN_H} (сейчас {}×{}).",
                    area.width, area.height
                ))
                .wrap(Wrap { trim: true }),
                area,
            );
            return;
        }
        self.draw_main(f, area);
    }

    fn title_line(&self) -> Line<'static> {
        let mut spans = vec![
            Span::styled(" ◆ ", Style::new().fg(Color::Cyan)),
            Span::styled(
                format!("Open Antigravity v{} ", update::current_version()),
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if crate::portable::enabled() {
                    "· портативный режим"
                } else {
                    "· установленная версия"
                },
                Style::new().fg(Color::DarkGray),
            ),
        ];
        if let Some(rel) = &self.update {
            let hint = if self.auto_update_running {
                "установка...".to_string()
            } else {
                "нажмите U для обновления".to_string()
            };
            spans.push(Span::styled(
                format!(
                    "  ⬆ новая версия {}: {} ",
                    rel.display_version(),
                    hint
                ),
                Style::new().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }
        Line::from(spans)
    }

    fn draw_main(&mut self, f: &mut Frame, area: Rect) {
        let facts = self.facts();
        let headline = facts.as_ref().map(status::headline);

        // The card's height follows its text, so a long detail is never cut.
        let inner_w = area.width.saturating_sub(4);
        let card_h = match &headline {
            Some(h) => 2 + 1 + wrapped_height(&h.detail, inner_w) + u16::from(self.busy.is_some()),
            None => 3,
        }
        .min(area.height / 3);
        let log_h = if area.height >= 32 { 9 } else { 6 };
        let [top, card, list, hint, journal, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(card_h),
            Constraint::Min(4),
            Constraint::Length(2),
            Constraint::Length(log_h),
            Constraint::Length(1),
        ])
        .areas(area);

        f.render_widget(Paragraph::new(self.title_line()), top);
        self.draw_card(f, card, headline.as_ref());

        let rows = self.rows();
        let (at, _) = self.cursor(&rows);
        self.list.select(Some(at));
        let items: Vec<ListItem> = rows.iter().map(|r| self.row_item(*r, list.width)).collect();
        let list_widget = List::new(items)
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("› ");
        f.render_stateful_widget(list_widget, list, &mut self.list);

        let selected = self.list.selected().and_then(|i| rows.get(i).copied());
        let hint_text = selected.map(|r| self.row_hint(r)).unwrap_or_default();
        f.render_widget(
            Paragraph::new(hint_text)
                .style(Style::new().fg(Color::DarkGray))
                .wrap(Wrap { trim: true }),
            hint,
        );

        self.draw_journal(f, journal);
        self.draw_keys(f, keys);

        if self.input.is_some() {
            self.draw_input(f, area);
        }
    }

    fn draw_card(&self, f: &mut Frame, area: Rect, headline: Option<&status::Headline>) {
        let Some(h) = headline else {
            f.render_widget(
                Paragraph::new("Проверяю систему…").block(
                    Block::new()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded),
                ),
                area,
            );
            return;
        };
        let (badge, color) = match h.tone {
            Tone::Ok => ("[ АКТИВЕН ]", Color::Green),
            Tone::Wait => ("[ ОЖИДАНИЕ ]", Color::Cyan),
            Tone::Fixing => ("[ ИСПРАВЛЕНИЕ ]", Color::Yellow),
            Tone::Action => ("[ ВНИМАНИЕ ]", Color::Red),
            Tone::Off => ("[ ВЫКЛЮЧЕН ]", Color::DarkGray),
        };
        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{badge} "), Style::new().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(h.title.clone(), Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
        ])];
        lines.push(Line::styled(format!("  {}", h.detail), Style::new().fg(Color::Gray)));
        if let Some(what) = &self.busy {
            lines.push(Line::styled(
                format!("  ⏳ {what}"),
                Style::new().fg(Color::Yellow),
            ));
        }
        f.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: true }).block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(color)),
            ),
            area,
        );
    }

    fn draw_journal(&self, f: &mut Frame, area: Rect) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Span::styled(" 📝 Журнал событий ", Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
            .border_style(Style::new().fg(Color::DarkGray));
        let inner = block.inner(area);
        f.render_widget(block, area);
        // Newest at the bottom; only as many as fit once wrapped.
        let mut lines: Vec<Line> = Vec::new();
        let mut used = 0u16;
        for (level, text) in self.log.iter().rev() {
            let shown = if *level == Level::Step {
                format!("— {text}")
            } else {
                text.clone()
            };
            let h = wrapped_height(&shown, inner.width);
            if used + h > inner.height {
                break;
            }
            used += h;
            let color = match level {
                Level::Ok => Color::Green,
                Level::Warn => Color::Yellow,
                Level::Err => Color::Red,
                Level::Step => Color::Cyan,
                Level::Info => Color::Reset,
            };
            lines.push(Line::styled(shown, Style::new().fg(color)));
        }
        lines.reverse();
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn draw_keys(&mut self, f: &mut Frame, area: Rect) {
        if let Some((text, at)) = &self.toast {
            if at.elapsed() < Duration::from_secs(6) {
                f.render_widget(
                    Paragraph::new(text.clone()).style(Style::new().fg(Color::Cyan)),
                    area,
                );
                return;
            }
            self.toast = None;
        }
        let offered = match self.facts().and_then(|f| status::headline(&f).action) {
            Some(Action::EnableAll) => " · a — активировать",
            Some(Action::Repair) => " · r — активировать",
            Some(Action::KillHolder) => " · e — завершить процесс",
            Some(Action::Verify) => " · v — пройти верификацию",
            _ => "",
        };
        let update_key = if self.update.is_some() {
            " · U — обновить"
        } else {
            " · u — обновления"
        };
        let keys = format!(
            "↑↓ выбор · Пробел/Enter — вкл/выкл{offered} · d — тест сети · s — ярлыки · t — автозапуск · o — отчёт{update_key} · q — выход"
        );
        f.render_widget(
            Paragraph::new(keys).style(Style::new().fg(Color::DarkGray)),
            area,
        );
    }

    fn draw_input(&self, f: &mut Frame, area: Rect) {
        let (title, hint, text, error) = match &self.input {
            Some(Input::Path { text, error }) => (
                " Путь к Antigravity ",
                "Папка установки Antigravity, IDE или CLI. Можно указать вложенную — корень будет найден сам.",
                text,
                error.as_deref(),
            ),
            Some(Input::OwnProxy { text }) => (
                " Свой HTTP-прокси ",
                "Формат: логин:пароль@адрес:порт — или просто адрес:порт, если пароля нет. \
                 Например ivan:secret@203.0.113.9:3128. Только HTTP-прокси. \
                 Пустая строка — убрать прокси.",
                text,
                None,
            ),
            None => return,
        };
        let w = area.width.saturating_sub(8).min(90);
        // Borders, the wrapped hint, a blank line, the field, the error, the keys.
        let h = 2 + wrapped_height(hint, w.saturating_sub(2)) + 3 + u16::from(error.is_some());
        let popup = Rect {
            x: area.x + (area.width - w) / 2,
            y: area.y + area.height.saturating_sub(h) / 2,
            width: w,
            height: h,
        };
        let mut lines = vec![
            Line::styled(hint, Style::new().fg(Color::Gray)),
            Line::raw(""),
            Line::from(vec![
                Span::raw("› "),
                Span::raw(text.clone()),
                Span::styled("▏", Style::new().fg(Color::Cyan)),
            ]),
        ];
        if let Some(e) = error {
            lines.push(Line::styled(e.to_string(), Style::new().fg(Color::Red)));
        }
        lines.push(Line::styled(
            "Enter — применить · Esc — отмена",
            Style::new().fg(Color::DarkGray),
        ));
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(title),
            ),
            popup,
        );
    }
}

/// One switch as a line: `[вкл ] Title — note`.
pub(super) fn switch_line(
    indent: &str,
    title: &str,
    state: &State,
    busy: bool,
    width: u16,
) -> ListItem<'static> {
    let (badge, color) = match state {
        State::On => (" ● ВКЛ ", Color::Green),
        State::Partial(_) => (" ◐ ЧАСТ", Color::Yellow),
        State::Off | State::OffNote(_) => (" ○ ВЫКЛ", Color::DarkGray),
        State::Blocked(_) => (" ⊘ БЛОК", Color::Red),
    };
    let color = if busy { Color::DarkGray } else { color };
    let mut spans = vec![
        Span::raw(indent.to_string()),
        Span::styled(badge, Style::new().fg(color)),
        Span::raw(format!(" {title}")),
    ];
    if let Some(note) = state.note() {
        let used = (indent.chars().count() + 7 + title.chars().count() + 3) as u16;
        let room = width.saturating_sub(used + 2) as usize;
        if room > 8 {
            let note: String = if note.chars().count() > room {
                note.chars()
                    .take(room - 1)
                    .chain(std::iter::once('…'))
                    .collect()
            } else {
                note.to_string()
            };
            let note_color = if matches!(state, State::Blocked(_)) {
                Color::Yellow
            } else {
                Color::Gray
            };
            spans.push(Span::styled(
                format!(" — {note}"),
                Style::new().fg(note_color),
            ));
        }
    }
    ListItem::new(Line::from(spans))
}

/// Lines `text` takes when wrapped to `width` columns — close enough to what
/// `Paragraph` does to size a box around it.
pub(super) fn wrapped_height(text: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    text.split('\n')
        .map(|line| {
            let n = line.chars().count();
            n.max(1).div_ceil(width)
        })
        .sum::<usize>()
        .min(usize::from(u16::MAX)) as u16
}

pub(super) enum Copied {
    Clipboard,
    Terminal,
}

/// Windows has a clipboard of its own to put the text on. Anywhere else this
/// may be a server with none, so the text goes to the terminal as OSC 52, which
/// a terminal that supports it copies on the machine the user sits at.
pub(super) fn copy_out(text: &str) -> Copied {
    #[cfg(windows)]
    if crate::utils::set_clipboard_text(text) {
        return Copied::Clipboard;
    }
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
    Copied::Terminal
}

pub(super) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

