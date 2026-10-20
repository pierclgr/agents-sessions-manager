//! terminal rendering and a testable state machine

use crate::{
    data::{
        Agent, DataPaths, Session, SessionKey,
        agents::{load_all_sessions, provider},
    },
    utils::{CommandRunner, ProcessRunner, SHORT_ID_LENGTH, format_datetime, truncate},
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::{
    collections::HashSet,
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const BANNER: [&str; 4] = [
    r"   _   ___ ___ _  _ _____ ___   ___ ___ ___ ___ ___ ___  _  _ ___   __  __   _   _  _   _   ___ ___ ___ ",
    r"  /_\ / __| __| \| |_   _/ __| / __| __/ __/ __|_ _/ _ \| \| / __| |  \/  | /_\ | \| | /_\ / __| __| _ \",
    r" / _ \ (_ | _|| .` | | | \__ \ \__ \ _|\__ \__ \| | (_) | .` \__ \ | |\/| |/ _ \| .` |/ _ \ (_ | _||   /",
    r"/_/ \_\___|___|_|\_| |_| |___/ |___/___|___/___/___\___/|_|\_|___/ |_|  |_/_/ \_\_|\_/_/ \_\___|___|_|_\",
];
const BANNER_HEIGHT: u16 = 5;
const MIN_WIDTH: u16 = 20;
const MIN_HEIGHT: u16 = 4;
const RIGHT_PANE_MIN_WIDTH: u16 = 60;
const NAME_MAX: usize = 24;
const RECENT_COMMAND_LIMIT: usize = 5;
const LEFT_PANE_PERCENT: u32 = 60;
const TRUE_COLOR_COUNT: u32 = 1 << 24;
const INDEXED_COLOR_COUNT: u32 = 256;
const MIN_COLORS_FOR_GREY: u32 = 16;
const SELECT_ACCENT: Color = Color::Rgb(202, 124, 94);
const DELETE_ACCENT: Color = Color::Rgb(55, 6, 3);
const COLUMN_GAP: usize = 2;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const SELECT_HINTS: &str = " MODE: SELECT  |  ↑/↓ Navigate  |  F: CWD/All  |  1-4: Filter  |  Enter: Resume session  |  D: Delete mode  |  ESC/Q: Quit";
const DELETE_HINTS: &str = " MODE: DELETE  |  ↑/↓ Navigate  |  F: CWD/All  |  1-4: Filter  |  Tab/Space: Toggle select  |  Shift+Tab: Select range  |  A: Select all  |  Enter: Delete selected  |  D: Select mode  |  ESC/Q: Quit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryScope {
    Cwd,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Select,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    None,
    Launch,
    Delete,
    Quit,
}

pub struct State {
    pub all_sessions: Vec<Session>,
    pub visible: Vec<usize>,
    pub filters: HashSet<Agent>,
    pub directory_scope: DirectoryScope,
    pub mode: Mode,
    pub cursor: usize,
    pub scroll_offset: usize,
    pub selected: HashSet<SessionKey>,
    pub range_anchor: Option<SessionKey>,
    pub message: Option<String>,
    pub confirming: bool,
    cwd: PathBuf,
    enriched_key: Option<SessionKey>,
}

impl State {
    /// start with sessions from the exact launch directory
    pub fn new(sessions: Vec<Session>, cwd: PathBuf) -> Self {
        let mut state = Self {
            visible: Vec::new(),
            all_sessions: sessions,
            filters: Agent::ALL.into_iter().collect(),
            directory_scope: DirectoryScope::Cwd,
            mode: Mode::Select,
            cursor: 0,
            scroll_offset: 0,
            selected: HashSet::new(),
            range_anchor: None,
            message: None,
            confirming: false,
            cwd,
            enriched_key: None,
        };
        state.apply_filter();
        state
    }

    pub fn current(&self) -> Option<&Session> {
        self.visible
            .get(self.cursor)
            .map(|index| &self.all_sessions[*index])
    }

    fn apply_filter(&mut self) {
        // match symlink aliases without changing paths used for display and launch
        let cwd = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());
        self.visible = self
            .all_sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| self.filters.contains(&session.agent))
            .filter(|(_, session)| {
                self.directory_scope == DirectoryScope::All
                    || session.project_path == self.cwd
                    || session
                        .project_path
                        .canonicalize()
                        .is_ok_and(|path| path == cwd)
            })
            .map(|(index, _)| index)
            .collect();
        let visible_keys: HashSet<_> = self
            .visible
            .iter()
            .map(|index| self.all_sessions[*index].key())
            .collect();
        self.selected.retain(|key| visible_keys.contains(key));
        self.cursor = self.cursor.min(self.visible.len().saturating_sub(1));
        self.scroll_offset = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        if self.confirming {
            self.confirming = false;
            self.message = None;
            return if key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
                && matches!(key.code, KeyCode::Char('y' | 'Y'))
            {
                Action::Delete
            } else {
                Action::None
            };
        }
        self.message = None;
        if !key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
            return Action::None;
        }
        if let KeyCode::Char(number @ '1'..='4') = key.code {
            let agent = Agent::ALL[(number as u8 - b'1') as usize];
            // pressing the key of the only shown agent shows all agents again
            if self.filters.len() == 1 && self.filters.contains(&agent) {
                self.filters = Agent::ALL.into_iter().collect();
            } else {
                self.filters = HashSet::from([agent]);
            }
            self.apply_filter();
            return Action::None;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q' | 'Q')) {
            return Action::Quit;
        }
        if matches!(key.code, KeyCode::Char('f' | 'F')) {
            self.directory_scope = if self.directory_scope == DirectoryScope::Cwd {
                DirectoryScope::All
            } else {
                DirectoryScope::Cwd
            };
            self.apply_filter();
            return Action::None;
        }
        if matches!(key.code, KeyCode::Char('d' | 'D')) {
            self.mode = if self.mode == Mode::Select {
                Mode::Delete
            } else {
                Mode::Select
            };
            if self.mode == Mode::Select {
                self.selected.clear();
                self.range_anchor = None;
            }
            return Action::None;
        }
        if self.visible.is_empty() {
            return Action::None;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(self.visible.len() - 1)
            }
            KeyCode::Enter if self.mode == Mode::Select => return Action::Launch,
            KeyCode::Enter => {
                if self.selected.is_empty() {
                    self.message = Some("No sessions selected.".into());
                } else {
                    self.confirming = true;
                    self.message =
                        Some(format!("Delete {} session(s)? [y/N]", self.selected.len()));
                }
            }
            KeyCode::Tab | KeyCode::Char(' ') if self.mode == Mode::Delete => {
                let key = self.current().unwrap().key();
                if !self.selected.remove(&key) {
                    self.selected.insert(key.clone());
                }
                self.range_anchor = Some(key);
            }
            KeyCode::BackTab if self.mode == Mode::Delete => {
                let keys: Vec<_> = self
                    .visible
                    .iter()
                    .map(|index| self.all_sessions[*index].key())
                    .collect();
                // only a selected anchor starts a range
                let anchor = self
                    .range_anchor
                    .as_ref()
                    .filter(|anchor| self.selected.contains(*anchor))
                    .and_then(|anchor| keys.iter().position(|key| key == anchor))
                    .unwrap_or(self.cursor);
                let first = anchor.min(self.cursor);
                let last = anchor.max(self.cursor);
                self.selected.extend(keys[first..=last].iter().cloned());
            }
            KeyCode::Char('a' | 'A') if self.mode == Mode::Delete => {
                let keys: HashSet<_> = self
                    .visible
                    .iter()
                    .map(|index| self.all_sessions[*index].key())
                    .collect();
                if self.selected == keys {
                    self.selected.clear();
                } else {
                    self.selected = keys;
                }
            }
            _ => {}
        }
        Action::None
    }

    pub fn delete_selected(
        &mut self,
        paths: &DataPaths,
        runner: &dyn CommandRunner,
    ) -> Result<(), String> {
        let selected: Vec<_> = self
            .visible
            .iter()
            .map(|index| &self.all_sessions[*index])
            .filter(|session| self.selected.contains(&session.key()))
            .collect();
        let keys: Vec<_> = selected.iter().map(|session| session.key()).collect();
        let mut errors = Vec::new();
        for agent in Agent::ALL {
            let sessions: Vec<_> = selected
                .iter()
                .copied()
                .filter(|session| session.agent == agent)
                .collect();
            if !sessions.is_empty() {
                errors.extend(provider(agent).delete_sessions(&sessions, paths, runner));
            }
        }
        self.all_sessions = load_all_sessions(paths)?;
        let remaining: HashSet<_> = self.all_sessions.iter().map(Session::key).collect();
        let deleted = keys.iter().filter(|key| !remaining.contains(key)).count();
        self.selected.clear();
        self.range_anchor = None;
        self.mode = Mode::Select;
        self.enriched_key = None;
        self.apply_filter();
        self.message = Some(format!(
            "Deleted {deleted} session(s).{}",
            errors
                .first()
                .map(|error| format!(" {} error(s): {error}", errors.len()))
                .unwrap_or_default()
        ));
        Ok(())
    }
}

pub struct Palette {
    colors: u32,
}

impl Palette {
    pub fn new(colors: u32) -> Self {
        Self { colors }
    }

    pub fn agent(&self, agent: Agent) -> Style {
        let color = if self.colors > INDEXED_COLOR_COUNT {
            match agent {
                Agent::Claude => Color::Rgb(202, 124, 94),
                Agent::Codex => Color::Rgb(135, 206, 250),
                Agent::OpenCode => Color::Rgb(144, 238, 144),
                Agent::Pi => Color::Rgb(255, 128, 255),
            }
        } else {
            match agent {
                Agent::Claude => Color::Red,
                Agent::Codex => Color::Cyan,
                Agent::OpenCode => Color::Green,
                Agent::Pi => Color::Magenta,
            }
        };
        Style::default().fg(color)
    }

    pub fn accent(&self, mode: Mode) -> Style {
        let background = if self.colors > INDEXED_COLOR_COUNT {
            if mode == Mode::Select {
                SELECT_ACCENT
            } else {
                DELETE_ACCENT
            }
        } else if mode == Mode::Delete {
            Color::Red
        } else if self.colors >= MIN_COLORS_FOR_GREY {
            Color::DarkGray
        } else {
            Color::White
        };
        let foreground = if self.colors < MIN_COLORS_FOR_GREY && mode == Mode::Select {
            Color::Black
        } else {
            Color::White
        };
        Style::default().fg(foreground).bg(background)
    }

    fn divider(&self, mode: Mode) -> Style {
        Style::default().fg(self.accent(mode).bg.unwrap_or(Color::Reset))
    }
}

pub fn render(frame: &mut Frame, state: &mut State, palette: &Palette) {
    let area = frame.area();
    if area.is_empty() {
        return;
    }
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        frame.render_widget(Paragraph::new("Terminal too small"), area);
        return;
    }
    let banner_height = if area.height > BANNER_HEIGHT + 5 {
        BANNER_HEIGHT
    } else {
        0
    };
    if banner_height > 0 {
        for (row, line) in BANNER.iter().enumerate() {
            frame.render_widget(
                Paragraph::new(*line)
                    .style(palette.divider(Mode::Select).add_modifier(Modifier::BOLD)),
                Rect::new(area.x, area.y + row as u16, area.width, 1),
            );
        }
    }
    let scope = if state.directory_scope == DirectoryScope::Cwd {
        "CWD"
    } else {
        "All"
    };
    let mut spans = vec![Span::styled(
        format!(" F: {scope}  |  Agents: "),
        Style::default().add_modifier(Modifier::BOLD),
    )];
    for (index, agent) in Agent::ALL.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        let marker = if state.filters.contains(&agent) {
            "[x]"
        } else {
            "[ ]"
        };
        spans.push(Span::raw(format!("{marker} {} ", index + 1)));
        spans.push(Span::styled(
            agent.label(),
            palette.agent(agent).add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(area.x, area.y + banner_height, area.width, 1),
    );
    let content = Rect::new(
        area.x,
        area.y + banner_height + 1,
        area.width,
        area.height - banner_height - 2,
    );
    let status = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    if state.visible.is_empty() {
        let text = "No sessions found.";
        let width = (text.len() as u16).min(content.width);
        frame.render_widget(
            Paragraph::new(text),
            Rect::new(
                content.x + (content.width - width) / 2,
                content.y + content.height / 2,
                width,
                1,
            ),
        );
        let mode_hint = if state.mode == Mode::Select {
            "D: Delete mode"
        } else {
            "D: Select mode"
        };
        frame.render_widget(
            Paragraph::new(format!(
                "  F: CWD/All  |  1-4: Filter  |  {mode_hint}  |  ESC/Q: Quit"
            ))
            .style(palette.accent(state.mode)),
            status,
        );
        return;
    }
    let data_height = content.height.saturating_sub(1) as usize;
    if state.cursor < state.scroll_offset {
        state.scroll_offset = state.cursor;
    } else if state.cursor >= state.scroll_offset + data_height {
        state.scroll_offset = (state.cursor + 1).saturating_sub(data_height);
    }
    let show_right = area.width >= RIGHT_PANE_MIN_WIDTH;
    let left_width = if show_right {
        (u32::from(area.width) * LEFT_PANE_PERCENT / 100) as u16
    } else {
        area.width
    };
    let left = Rect::new(content.x, content.y, left_width, content.height);
    draw_sessions(frame, state, palette, left);
    if show_right {
        let divider = content.x + left_width;
        for row in content.y..content.y + content.height {
            frame.render_widget(
                Paragraph::new("│").style(palette.divider(state.mode)),
                Rect::new(divider, row, 1, 1),
            );
        }
        let right = Rect::new(
            divider + 1,
            content.y,
            area.width - left_width - 1,
            content.height,
        );
        draw_detail(frame, state.current().unwrap(), right);
    }
    let hints = if state.mode == Mode::Select {
        SELECT_HINTS
    } else {
        DELETE_HINTS
    };
    let text = state
        .message
        .as_ref()
        .map(|text| format!(" {text}"))
        .unwrap_or_else(|| hints.into());
    frame.render_widget(
        Paragraph::new(text).style(palette.accent(state.mode).add_modifier(Modifier::BOLD)),
        status,
    );
}

fn draw_sessions(frame: &mut Frame, state: &State, palette: &Palette, area: Rect) {
    let marker_width = if state.mode == Mode::Delete {
        4usize
    } else {
        0
    };
    let headers = ["Agent", "Project", "Session", "Modified", "Last message"];
    let mut widths = [
        headers[0].len(),
        headers[1].len(),
        headers[2].len(),
        headers[3].len(),
    ];
    let mut rows = Vec::new();
    for (row, index) in state
        .visible
        .iter()
        .skip(state.scroll_offset)
        .take(area.height.saturating_sub(1) as usize)
        .enumerate()
    {
        let session = &state.all_sessions[*index];
        let project = session
            .project_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let label = session
            .name
            .as_ref()
            .map(|name| truncate(name, NAME_MAX))
            .unwrap_or_else(|| session.session_id.chars().take(SHORT_ID_LENGTH).collect());
        let values = [
            session.agent.key().to_owned(),
            project,
            label,
            format_datetime(session.last_active_at),
        ];
        for (width, value) in widths.iter_mut().zip(&values) {
            *width = (*width).max(value.chars().count());
        }
        rows.push((row, session, values));
    }
    let header = format!(
        "{}{:w0$}  {:w1$}  {:w2$}  {:w3$}  {}",
        " ".repeat(marker_width),
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        headers[4],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3]
    );
    frame.render_widget(
        Paragraph::new(header).style(palette.accent(state.mode).add_modifier(Modifier::BOLD)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let fixed = widths.iter().sum::<usize>() + COLUMN_GAP * 4 + marker_width;
    let command_width = (area.width as usize).saturating_sub(fixed).max(10);
    for (row, session, values) in rows {
        let highlighted = state.scroll_offset + row == state.cursor;
        let style = if highlighted {
            palette.accent(state.mode)
        } else {
            Style::default()
        };
        let mut spans = Vec::new();
        if marker_width > 0 {
            spans.push(Span::raw(if state.selected.contains(&session.key()) {
                "[x] "
            } else {
                "[ ] "
            }));
        }
        spans.push(Span::styled(
            values[0].clone(),
            if highlighted {
                style
            } else {
                palette.agent(session.agent)
            },
        ));
        spans.push(Span::raw(format!(
            "{}  {:w1$}  {:w2$}  {:w3$}  {}",
            " ".repeat(widths[0].saturating_sub(values[0].chars().count())),
            values[1],
            values[2],
            values[3],
            truncate(session.latest_command.trim(), command_width),
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3]
        )));
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(style),
            Rect::new(area.x, area.y + 1 + row as u16, area.width, 1),
        );
    }
}

fn draw_detail(frame: &mut Frame, session: &Session, area: Rect) {
    if area.width < 2 {
        return;
    }
    let content = Rect::new(area.x + 1, area.y, area.width - 2, area.height);
    let mut lines = vec![
        "Agent:".into(),
        format!("  {}", session.agent.key()),
        String::new(),
        "Project:".into(),
        format!("  {}", session.project_path.display()),
        String::new(),
    ];
    if let Some(name) = &session.name {
        lines.extend(["Name:".into(), format!("  {name}"), String::new()]);
    }
    lines.extend([
        "Session ID:".into(),
        format!("  {}", session.session_id),
        String::new(),
        "Created:".into(),
        format!("  {}", format_datetime(session.created_at)),
        String::new(),
        "Last active:".into(),
        format!("  {}", format_datetime(session.last_active_at)),
        String::new(),
        "Latest commands:".into(),
    ]);
    if session.commands.is_empty() {
        lines.push("  (none)".into());
    }
    for (number, command) in session
        .commands
        .iter()
        .rev()
        .take(RECENT_COMMAND_LIMIT)
        .enumerate()
    {
        lines.push(format!(
            "  {}. {}",
            number + 1,
            truncate(command.trim(), (content.width as usize).saturating_sub(6))
        ));
    }
    lines.extend([String::new(), "Todos:".into()]);
    if session.todos.is_empty() {
        lines.push("  (none)".into());
    }
    for todo in &session.todos {
        lines.push(format!(
            "  - {}",
            truncate(todo, (content.width as usize).saturating_sub(4))
        ));
    }
    frame.render_widget(Paragraph::new(lines.join("\n")), content);
}

/// leave the alternate screen before the caller replaces this process
pub fn run(paths: &DataPaths, sessions: Vec<Session>) -> Result<Option<Session>, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("Error: Cannot read the launch directory: {error}"))?;
    let colors = crossterm::style::available_color_count();
    let palette = Palette::new(if colors == u16::MAX {
        TRUE_COLOR_COUNT
    } else {
        u32::from(colors)
    });
    let mut state = State::new(sessions, cwd);
    let interrupted = Arc::new(AtomicBool::new(false));
    let mut guard = TerminalGuard { signals: vec![] };
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        guard.signals.push(
            signal_hook::flag::register(signal, Arc::clone(&interrupted))
                .map_err(|error| format!("Error: {error}"))?,
        );
    }
    let mut terminal =
        ratatui::try_init().map_err(|error| format!("Error: Cannot open the terminal: {error}"))?;
    let result = (|| -> io::Result<Option<Session>> {
        loop {
            if interrupted.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if let Some(index) = state.visible.get(state.cursor).copied() {
                let session = &mut state.all_sessions[index];
                if state.enriched_key.as_ref() != Some(&session.key()) {
                    provider(session.agent).load_detail(session, paths);
                    state.enriched_key = Some(session.key());
                }
            }
            terminal.draw(|frame| render(frame, &mut state, &palette))?;
            if !event::poll(POLL_INTERVAL)? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            match state.handle_key(key) {
                Action::Quit => return Ok(None),
                Action::Launch => {
                    let session = state.current().unwrap();
                    if session.project_path.is_dir() {
                        return Ok(Some(session.clone()));
                    }
                    state.message = Some(format!(
                        "Error: Directory '{}' no longer exists.",
                        session.project_path.display()
                    ));
                }
                Action::Delete => state
                    .delete_selected(paths, &ProcessRunner)
                    .map_err(io::Error::other)?,
                Action::None => {}
            }
        }
    })();
    drop(guard);
    result.map_err(|error| format!("Error: {error}"))
}

struct TerminalGuard {
    signals: Vec<signal_hook::SigId>,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
        for signal in self.signals.drain(..) {
            signal_hook::low_level::unregister(signal);
        }
    }
}
