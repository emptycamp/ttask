//! Built-in form-based terminal editor for tasks.
//!
//! Four labelled fields (text, category, ord, est), navigable with Tab/↑/↓. Enter
//! advances to the next field. `:` drops into a vim-style command mode where the user
//! can run `:w`, `:wq`, `:q`, `:q!`. Esc and Ctrl+C are intentionally inert in edit
//! mode — exiting is explicit (`:q`, `:wq`, `:q!`).

use crate::editor::Saver;
use crate::error::{Error, Result};
use crate::model::{Category, Task};
use crate::time::parse_duration;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use std::io;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Field {
    Text,
    Category,
    Ord,
    Est,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Self::Text => Self::Category,
            Self::Category => Self::Ord,
            Self::Ord => Self::Est,
            Self::Est => Self::Text,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Text => Self::Est,
            Self::Category => Self::Text,
            Self::Ord => Self::Category,
            Self::Est => Self::Ord,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Edit,
    Command,
}

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    text: String,
    category: Category,
    ord_str: String,
    est_str: String,
    text_cursor: usize,
    ord_cursor: usize,
    est_cursor: usize,
}

#[derive(Clone, Debug)]
pub struct State {
    pub task_id: u32,
    pub text: String,
    pub text_cursor: usize,
    pub category: Category,
    pub ord_str: String,
    pub ord_cursor: usize,
    pub est_str: String,
    pub est_cursor: usize,
    pub focus: Field,
    pub error: Option<String>,
    pub status: Option<String>,
    pub mode: Mode,
    pub command_buf: String,
    pub ord_pristine: bool,
    pub est_pristine: bool,
    pub dirty: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    original: Snapshot,
}

impl State {
    pub fn from_task(task: &Task) -> Self {
        let ord_str = task.ord.to_string();
        let est_str = format_est(task.est_secs);
        let original = Snapshot {
            text: task.text.clone(),
            category: task.category,
            ord_str: ord_str.clone(),
            est_str: est_str.clone(),
            text_cursor: task.text.chars().count(),
            ord_cursor: ord_str.chars().count(),
            est_cursor: est_str.chars().count(),
        };
        Self {
            task_id: task.id,
            text: task.text.clone(),
            text_cursor: task.text.chars().count(),
            category: task.category,
            ord_cursor: ord_str.chars().count(),
            ord_str,
            est_cursor: est_str.chars().count(),
            est_str,
            focus: Field::Text,
            error: None,
            status: None,
            mode: Mode::Edit,
            command_buf: String::new(),
            ord_pristine: false,
            est_pristine: false,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            original,
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            category: self.category,
            ord_str: self.ord_str.clone(),
            est_str: self.est_str.clone(),
            text_cursor: self.text_cursor,
            ord_cursor: self.ord_cursor,
            est_cursor: self.est_cursor,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.text = s.text;
        self.category = s.category;
        self.ord_str = s.ord_str;
        self.est_str = s.est_str;
        self.text_cursor = s.text_cursor;
        self.ord_cursor = s.ord_cursor;
        self.est_cursor = s.est_cursor;
    }

    fn checkpoint(&mut self) {
        let snap = self.snapshot();
        if self.undo_stack.last() != Some(&snap) {
            self.undo_stack.push(snap);
        }
        self.redo_stack.clear();
        self.dirty = self.snapshot() != self.original;
        self.error = None;
    }

    fn recompute_dirty(&mut self) {
        self.dirty = self.snapshot() != self.original;
    }

    pub fn commit(&self, baseline: &Task) -> std::result::Result<Task, String> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return Err("text cannot be empty".into());
        }
        let ord: u32 = match self.ord_str.trim().parse() {
            Ok(n) if n >= 1 => n,
            Ok(_) => return Err("ord must be >= 1".into()),
            Err(_) => return Err(format!("invalid ord '{}'", self.ord_str.trim())),
        };
        let est_secs = match parse_duration(self.est_str.trim()) {
            Ok(d) => d.num_seconds(),
            Err(e) => return Err(format!("invalid est: {e}")),
        };

        let mut updated = baseline.clone();
        updated.text = trimmed.to_string();
        updated.category = self.category;
        updated.ord = ord;
        updated.est_secs = est_secs;
        Ok(updated)
    }

    fn undo(&mut self) {
        let Some(prev) = self.undo_stack.pop() else {
            self.status = Some("nothing to undo".into());
            return;
        };
        let curr = self.snapshot();
        self.redo_stack.push(curr);
        self.restore(prev);
        self.recompute_dirty();
        self.status = Some("undo".into());
        self.error = None;
    }

    fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            self.status = Some("nothing to redo".into());
            return;
        };
        let curr = self.snapshot();
        self.undo_stack.push(curr);
        self.restore(next);
        self.recompute_dirty();
        self.status = Some("redo".into());
        self.error = None;
    }

    fn rebaseline(&mut self, persisted: &Task) {
        self.ord_str = persisted.ord.to_string();
        self.ord_cursor = self.ord_str.chars().count();
        self.original = self.snapshot();
        self.dirty = false;
        self.task_id = persisted.id;
    }
}

fn format_est(secs: i64) -> String {
    if secs % 3600 == 0 && secs > 0 {
        format!("{}h", secs / 3600)
    } else if secs % 60 == 0 && secs > 0 {
        format!("{}m", secs / 60)
    } else if secs <= 0 {
        "0m".to_string()
    } else {
        format!("{secs}s")
    }
}

pub enum Action {
    Continue,
    Save,
    SaveAndQuit,
    Cancel,
}

pub fn handle_key(state: &mut State, key: KeyEvent) -> Action {
    if state.mode == Mode::Command {
        return handle_command_mode(state, key);
    }

    if matches!(key.code, KeyCode::Esc)
        || matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('c'), KeyModifiers::CONTROL)
        )
    {
        return Action::Continue;
    }

    match (key.code, key.modifiers) {
        (KeyCode::Char('z'), m)
            if m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::SHIFT) =>
        {
            state.undo();
            return Action::Continue;
        }
        (KeyCode::Char('Z'), m)
            if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
        {
            state.redo();
            return Action::Continue;
        }
        (KeyCode::Char('y'), m) if m.contains(KeyModifiers::CONTROL) => {
            state.redo();
            return Action::Continue;
        }
        (KeyCode::Char(':'), KeyModifiers::NONE) | (KeyCode::Char(':'), KeyModifiers::SHIFT) => {
            state.mode = Mode::Command;
            state.command_buf = String::new();
            state.error = None;
            state.status = None;
            return Action::Continue;
        }
        (KeyCode::Tab, _) | (KeyCode::Down, _) => {
            move_focus(state, true);
            return Action::Continue;
        }
        (KeyCode::BackTab, _) | (KeyCode::Up, _) => {
            move_focus(state, false);
            return Action::Continue;
        }
        (KeyCode::Enter, _) => {
            move_focus(state, true);
            return Action::Continue;
        }
        _ => {}
    }

    match state.focus {
        Field::Text => handle_text_field(state, key),
        Field::Ord => handle_text_ord(state, key),
        Field::Est => handle_text_est(state, key),
        Field::Category => handle_category(state, key),
    }
    Action::Continue
}

fn handle_command_mode(state: &mut State, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            state.mode = Mode::Edit;
            state.command_buf.clear();
            Action::Continue
        }
        KeyCode::Backspace => {
            if state.command_buf.is_empty() {
                state.mode = Mode::Edit;
            } else {
                state.command_buf.pop();
            }
            Action::Continue
        }
        KeyCode::Enter => execute_command(state),
        KeyCode::Char(c) => {
            state.command_buf.push(c);
            Action::Continue
        }
        _ => Action::Continue,
    }
}

fn execute_command(state: &mut State) -> Action {
    let cmd = state.command_buf.trim().to_string();
    state.mode = Mode::Edit;
    state.command_buf.clear();
    match cmd.as_str() {
        "w" => Action::Save,
        "wq" | "x" => Action::SaveAndQuit,
        "q" => {
            if state.dirty {
                state.error = Some("unsaved changes — use :wq to save, :q! to discard".into());
                Action::Continue
            } else {
                Action::Cancel
            }
        }
        "q!" => Action::Cancel,
        other => {
            state.error = Some(format!("unknown command :{other}"));
            Action::Continue
        }
    }
}

fn move_focus(state: &mut State, forward: bool) {
    state.focus = if forward {
        state.focus.next()
    } else {
        state.focus.prev()
    };
    state.ord_pristine = matches!(state.focus, Field::Ord);
    state.est_pristine = matches!(state.focus, Field::Est);
}

fn handle_text_field(state: &mut State, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => {
            state.checkpoint();
            let mut cursor = state.text_cursor;
            insert_char(&mut state.text, &mut cursor, c);
            state.text_cursor = cursor;
            state.recompute_dirty();
        }
        KeyCode::Backspace => {
            state.checkpoint();
            let mut cursor = state.text_cursor;
            delete_before(&mut state.text, &mut cursor);
            state.text_cursor = cursor;
            state.recompute_dirty();
        }
        KeyCode::Delete => {
            state.checkpoint();
            let mut cursor = state.text_cursor;
            delete_at(&mut state.text, &mut cursor);
            state.text_cursor = cursor;
            state.recompute_dirty();
        }
        KeyCode::Left => {
            state.text_cursor = state.text_cursor.saturating_sub(1);
        }
        KeyCode::Right => {
            let len = state.text.chars().count();
            if state.text_cursor < len {
                state.text_cursor += 1;
            }
        }
        KeyCode::Home => state.text_cursor = 0,
        KeyCode::End => state.text_cursor = state.text.chars().count(),
        _ => {}
    }
}

fn handle_text_ord(state: &mut State, key: KeyEvent) {
    if matches!(key.code, KeyCode::Char(_)) && state.ord_pristine {
        state.checkpoint();
        state.ord_str.clear();
        state.ord_cursor = 0;
        state.ord_pristine = false;
    }
    match key.code {
        KeyCode::Char(c) if c.is_ascii_digit() => {
            state.checkpoint();
            insert_char(&mut state.ord_str, &mut state.ord_cursor, c);
            state.recompute_dirty();
        }
        KeyCode::Backspace => {
            state.checkpoint();
            delete_before(&mut state.ord_str, &mut state.ord_cursor);
            state.recompute_dirty();
        }
        KeyCode::Delete => {
            state.checkpoint();
            delete_at(&mut state.ord_str, &mut state.ord_cursor);
            state.recompute_dirty();
        }
        KeyCode::Left => {
            state.ord_cursor = state.ord_cursor.saturating_sub(1);
        }
        KeyCode::Right => {
            let len = state.ord_str.chars().count();
            if state.ord_cursor < len {
                state.ord_cursor += 1;
            }
        }
        KeyCode::Home => state.ord_cursor = 0,
        KeyCode::End => state.ord_cursor = state.ord_str.chars().count(),
        _ => {}
    }
}

fn handle_text_est(state: &mut State, key: KeyEvent) {
    if matches!(key.code, KeyCode::Char(_)) && state.est_pristine {
        state.checkpoint();
        state.est_str.clear();
        state.est_cursor = 0;
        state.est_pristine = false;
    }
    match key.code {
        KeyCode::Char(c) => {
            state.checkpoint();
            insert_char(&mut state.est_str, &mut state.est_cursor, c);
            state.recompute_dirty();
        }
        KeyCode::Backspace => {
            state.checkpoint();
            delete_before(&mut state.est_str, &mut state.est_cursor);
            state.recompute_dirty();
        }
        KeyCode::Delete => {
            state.checkpoint();
            delete_at(&mut state.est_str, &mut state.est_cursor);
            state.recompute_dirty();
        }
        KeyCode::Left => {
            state.est_cursor = state.est_cursor.saturating_sub(1);
        }
        KeyCode::Right => {
            let len = state.est_str.chars().count();
            if state.est_cursor < len {
                state.est_cursor += 1;
            }
        }
        KeyCode::Home => state.est_cursor = 0,
        KeyCode::End => state.est_cursor = state.est_str.chars().count(),
        _ => {}
    }
}

fn insert_char(text: &mut String, cursor: &mut usize, c: char) {
    let byte_idx = char_to_byte_idx(text, *cursor);
    text.insert(byte_idx, c);
    *cursor += 1;
}

fn delete_before(text: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let byte_idx = char_to_byte_idx(text, *cursor - 1);
    text.remove(byte_idx);
    *cursor -= 1;
}

fn delete_at(text: &mut String, cursor: &mut usize) {
    let len = text.chars().count();
    if *cursor >= len {
        return;
    }
    let byte_idx = char_to_byte_idx(text, *cursor);
    text.remove(byte_idx);
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn handle_category(state: &mut State, key: KeyEvent) {
    let new_category = match key.code {
        KeyCode::Char('a') | KeyCode::Char('A') => Some(Category::A),
        KeyCode::Char('b') | KeyCode::Char('B') => Some(Category::B),
        KeyCode::Char('c') | KeyCode::Char('C') => Some(Category::C),
        KeyCode::Left => Some(match state.category {
            Category::A => Category::C,
            Category::B => Category::A,
            Category::C => Category::B,
        }),
        KeyCode::Right | KeyCode::Char(' ') => Some(match state.category {
            Category::A => Category::B,
            Category::B => Category::C,
            Category::C => Category::A,
        }),
        _ => None,
    };
    if let Some(p) = new_category {
        if p != state.category {
            state.checkpoint();
            state.category = p;
            state.recompute_dirty();
        }
    }
}

fn draw(frame: &mut Frame, state: &State) {
    let area = frame.area();
    let title = if state.task_id == 0 {
        " New Task ".to_string()
    } else if state.dirty {
        format!(" Edit Task #{}  ●  unsaved ", state.task_id)
    } else {
        format!(" Edit Task #{} ", state.task_id)
    };
    let outer = Block::default().borders(Borders::ALL).title(title);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    draw_field(
        frame,
        chunks[1],
        "Text:    ",
        &state.text,
        state.focus == Field::Text && state.mode == Mode::Edit,
        state.text_cursor,
    );
    draw_category(
        frame,
        chunks[2],
        state.category,
        state.focus == Field::Category && state.mode == Mode::Edit,
    );
    draw_field(
        frame,
        chunks[3],
        "Ord:     ",
        &state.ord_str,
        state.focus == Field::Ord && state.mode == Mode::Edit,
        state.ord_cursor,
    );
    draw_field(
        frame,
        chunks[4],
        "Est:     ",
        &state.est_str,
        state.focus == Field::Est && state.mode == Mode::Edit,
        state.est_cursor,
    );

    let status_line: Line = if state.mode == Mode::Command {
        Line::from(Span::styled(
            format!(" :{}", state.command_buf),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
    } else if let Some(err) = &state.error {
        Line::from(Span::styled(
            format!(" ! {err}"),
            Style::default().fg(Color::Red),
        ))
    } else if let Some(status) = &state.status {
        Line::from(Span::styled(
            format!(" {status}"),
            Style::default().fg(Color::Green),
        ))
    } else {
        Line::from(Span::raw(""))
    };
    frame.render_widget(Paragraph::new(status_line), chunks[6]);

    let help_text = if state.mode == Mode::Command {
        " :w save · :wq save & quit · :q quit · :q! discard · Esc back "
    } else {
        " Tab/↑↓ next · Enter next · : command · Ctrl+Z undo · Ctrl+Shift+Z redo "
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            help_text,
            Style::default().fg(Color::DarkGray),
        )),
        chunks[8],
    );

    if state.mode == Mode::Command {
        let x = inner.x + 2 + state.command_buf.chars().count() as u16;
        frame.set_cursor_position((x.min(inner.x + inner.width.saturating_sub(1)), chunks[6].y));
    }
}

fn draw_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    cursor: usize,
) {
    let label_span = Span::styled(label.to_string(), Style::default().fg(Color::Cyan));
    let value_style = if focused {
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let display_width = area.width.saturating_sub(label.chars().count() as u16 + 2) as usize;
    let mut padded = value.to_string();
    while padded.chars().count() < display_width {
        padded.push(' ');
    }
    let value_span = Span::styled(padded, value_style);

    let line = Line::from(vec![Span::raw(" "), label_span, value_span]);
    frame.render_widget(Paragraph::new(line), area);

    if focused {
        let x = area.x + 1 + label.chars().count() as u16 + cursor as u16;
        let y = area.y;
        frame.set_cursor_position((x.min(area.x + area.width.saturating_sub(1)), y));
    }
}

fn draw_category(frame: &mut Frame, area: Rect, category: Category, focused: bool) {
    let label = Span::styled("Category:", Style::default().fg(Color::Cyan));
    let mut spans = vec![Span::raw(" "), label, Span::raw(" ")];
    for p in [Category::A, Category::B, Category::C] {
        let selected = p == category;
        let style = if selected && focused {
            Style::default()
                .fg(Color::Black)
                .bg(category_color(p))
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default()
                .fg(category_color(p))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(format!(" {p} "), style));
        spans.push(Span::raw(" "));
    }
    if focused {
        spans.push(Span::styled(
            "  (←→ to cycle, A/B/C to set)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn category_color(p: Category) -> Color {
    match p {
        Category::A => Color::Red,
        Category::B => Color::Yellow,
        Category::C => Color::DarkGray,
    }
}

pub fn run(task: &Task, save: &mut Saver<'_>) -> Result<()> {
    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::Parse(
            "the form editor requires a TTY; pass field args instead, e.g. `task edit <id> c:a` or `task edit <id> ord:1`".into(),
        ));
    }
    enable_raw_mode().map_err(Error::Io)?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(Error::Io)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(Error::Io)?;

    let result = run_loop(&mut terminal, task, save);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    initial: &Task,
    save: &mut Saver<'_>,
) -> Result<()> {
    let mut baseline = initial.clone();
    let mut state = State::from_task(&baseline);
    loop {
        terminal.draw(|f| draw(f, &state)).map_err(Error::Io)?;

        let event = event::read().map_err(Error::Io)?;
        let key = match event {
            Event::Key(k) if k.kind == KeyEventKind::Press => k,
            _ => continue,
        };

        match handle_key(&mut state, key) {
            Action::Continue => {}
            Action::Cancel => return Ok(()),
            Action::Save => match state.commit(&baseline) {
                Ok(updated) => match save(updated) {
                    Ok(persisted) => {
                        baseline = persisted.clone();
                        state.rebaseline(&persisted);
                        state.status = Some("saved".into());
                    }
                    Err(e) => state.error = Some(format!("save failed: {e}")),
                },
                Err(msg) => state.error = Some(msg),
            },
            Action::SaveAndQuit => match state.commit(&baseline) {
                Ok(updated) => match save(updated) {
                    Ok(_) => return Ok(()),
                    Err(e) => state.error = Some(format!("save failed: {e}")),
                },
                Err(msg) => state.error = Some(msg),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status};
    use chrono::Utc;

    fn make_task() -> Task {
        let now = Utc::now();
        Task {
            id: 1,
            text: "Buy milk".to_string(),
            category: Category::B,
            ord: 1,
            est_secs: 1800,
            status: Status::Active,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn from_task_seeds_ord_as_string() {
        let mut t = make_task();
        t.ord = 7;
        let state = State::from_task(&t);
        assert_eq!(state.ord_str, "7");
    }

    #[test]
    fn esc_is_noop_in_edit_mode() {
        let task = make_task();
        let mut state = State::from_task(&task);
        let action = handle_key(&mut state, key(KeyCode::Esc));
        assert!(matches!(action, Action::Continue));
    }

    #[test]
    fn ctrl_c_is_noop_in_edit_mode() {
        let task = make_task();
        let mut state = State::from_task(&task);
        let action = handle_key(&mut state, ctrl(KeyCode::Char('c')));
        assert!(matches!(action, Action::Continue));
    }

    #[test]
    fn tab_cycles_focus_forward() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Category);
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Ord);
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Est);
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Text);
    }

    #[test]
    fn typing_in_text_inserts_at_cursor() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Char('!')));
        assert_eq!(state.text, "Buy milk!");
        assert!(state.dirty);
    }

    #[test]
    fn ord_field_accepts_only_digits() {
        let mut t = make_task();
        t.ord = 1;
        let mut state = State::from_task(&t);
        handle_key(&mut state, key(KeyCode::Tab));
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Ord);
        handle_key(&mut state, key(KeyCode::Char('5')));
        assert_eq!(state.ord_str, "5");
        // Non-digit is rejected.
        handle_key(&mut state, key(KeyCode::Char('x')));
        assert_eq!(state.ord_str, "5");
    }

    #[test]
    fn colon_wq_signals_save_and_quit() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
        );
        handle_key(&mut state, key(KeyCode::Char('w')));
        handle_key(&mut state, key(KeyCode::Char('q')));
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(action, Action::SaveAndQuit));
    }

    #[test]
    fn colon_q_errors_when_dirty() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Char('!')));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
        );
        handle_key(&mut state, key(KeyCode::Char('q')));
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(action, Action::Continue));
        assert!(state.error.is_some());
    }

    #[test]
    fn colon_q_when_clean_cancels() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
        );
        handle_key(&mut state, key(KeyCode::Char('q')));
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(action, Action::Cancel));
    }

    #[test]
    fn ctrl_z_undoes_text_change() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Char('!')));
        assert_eq!(state.text, "Buy milk!");
        handle_key(&mut state, ctrl(KeyCode::Char('z')));
        assert_eq!(state.text, "Buy milk");
    }

    #[test]
    fn commit_returns_ord_as_typed() {
        let mut task = make_task();
        task.ord = 3;
        let mut state = State::from_task(&task);
        // Tab to Ord, replace value.
        handle_key(&mut state, key(KeyCode::Tab));
        handle_key(&mut state, key(KeyCode::Tab));
        handle_key(&mut state, key(KeyCode::Char('9')));
        let updated = state.commit(&task).unwrap();
        assert_eq!(updated.ord, 9);
    }
}
