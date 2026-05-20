//! Built-in form-based terminal editor for tasks.
//!
//! Four labelled fields (text, priority, due, est), navigable with Tab/↑/↓. Enter
//! advances to the next field. `:` drops into a vim-style command mode where the user
//! can run `:w`, `:wq`, `:q`, `:q!`. Esc and Ctrl+C are intentionally inert in edit
//! mode — exiting is explicit (`:q`, `:wq`, `:q!`).
//!
//! Other affordances:
//! - Ctrl+Z / Ctrl+Shift+Z (also Ctrl+Y) undo/redo field edits.
//! - When you tab into Due or Est the next typed character clears the field first,
//!   so you can replace its value without backspacing.
//! - `:w` persists the task immediately via the supplied save callback. After a
//!   successful save the dirty flag clears and the baseline rolls forward, so a
//!   subsequent `:q` exits cleanly.

use crate::editor::Saver;
use crate::error::{Error, Result};
use crate::format::format_relative;
use crate::model::{Priority, Task};
use crate::time::{parse_due, parse_duration};
use chrono::Local;
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
    Priority,
    Due,
    Est,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Self::Text => Self::Priority,
            Self::Priority => Self::Due,
            Self::Due => Self::Est,
            Self::Est => Self::Text,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Text => Self::Est,
            Self::Priority => Self::Text,
            Self::Due => Self::Priority,
            Self::Est => Self::Due,
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
    priority: Priority,
    due_str: String,
    est_str: String,
    text_cursor: usize,
    due_cursor: usize,
    est_cursor: usize,
}

#[derive(Clone, Debug)]
pub struct State {
    pub task_id: u32,
    pub text: String,
    pub text_cursor: usize,
    pub priority: Priority,
    pub due_str: String,
    pub due_cursor: usize,
    pub est_str: String,
    pub est_cursor: usize,
    pub focus: Field,
    pub error: Option<String>,
    pub status: Option<String>,
    pub mode: Mode,
    pub command_buf: String,
    pub due_pristine: bool,
    pub est_pristine: bool,
    pub dirty: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    /// Snapshot of the most recently persisted (or initial) state. Used both for the
    /// dirty check and to decide whether to re-parse Due — when the displayed string
    /// matches the baseline exactly, we use the baseline's stored DateTime so the user
    /// doesn't get drift from "in 5m" being parsed at commit time.
    original: Snapshot,
}

impl State {
    pub fn from_task(task: &Task) -> Self {
        let due_str = format_relative(task.due, chrono::Utc::now());
        let est_str = format_est(task.est_secs);
        let original = Snapshot {
            text: task.text.clone(),
            priority: task.priority,
            due_str: due_str.clone(),
            est_str: est_str.clone(),
            text_cursor: task.text.chars().count(),
            due_cursor: due_str.chars().count(),
            est_cursor: est_str.chars().count(),
        };
        Self {
            task_id: task.id,
            text: task.text.clone(),
            text_cursor: task.text.chars().count(),
            priority: task.priority,
            due_cursor: due_str.chars().count(),
            due_str,
            est_cursor: est_str.chars().count(),
            est_str,
            focus: Field::Text,
            error: None,
            status: None,
            mode: Mode::Edit,
            command_buf: String::new(),
            due_pristine: false,
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
            priority: self.priority,
            due_str: self.due_str.clone(),
            est_str: self.est_str.clone(),
            text_cursor: self.text_cursor,
            due_cursor: self.due_cursor,
            est_cursor: self.est_cursor,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.text = s.text;
        self.priority = s.priority;
        self.due_str = s.due_str;
        self.est_str = s.est_str;
        self.text_cursor = s.text_cursor;
        self.due_cursor = s.due_cursor;
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

    /// Build a Task by overlaying form values onto `baseline`. Returns Err with a
    /// user-facing message on validation failure (empty text, unparseable due/est).
    ///
    /// If the Due field hasn't been edited (compared verbatim against the baseline's
    /// displayed form), we re-use baseline.due directly — this keeps "in 5m" from
    /// drifting forward when the user :w's without touching the Due field.
    pub fn commit(&self, baseline: &Task) -> std::result::Result<Task, String> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return Err("text cannot be empty".into());
        }
        let due = if self.due_str == self.original.due_str {
            baseline.due
        } else {
            let now_local: chrono::DateTime<Local> = chrono::Utc::now().into();
            match parse_due(self.due_str.trim(), now_local) {
                Ok(d) => d.with_timezone(&chrono::Utc),
                Err(e) => return Err(format!("invalid due: {e}")),
            }
        };
        let est_secs = match parse_duration(self.est_str.trim()) {
            Ok(d) => d.num_seconds(),
            Err(e) => return Err(format!("invalid est: {e}")),
        };

        let mut updated = baseline.clone();
        updated.text = trimmed.to_string();
        updated.priority = self.priority;
        updated.due = due;
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

    /// Refresh the baseline after a successful save. The current form contents become
    /// the new "clean" point — :q now exits without complaining.
    fn rebaseline(&mut self, persisted: &Task) {
        // Update due_str to mirror the persisted due so future :w without edits keeps
        // working off baseline.due. Without this, the displayed "in 5m" could lag the
        // persisted DateTime and a no-op :w would parse and shift things.
        let new_due_str = format_relative(persisted.due, chrono::Utc::now());
        self.due_str = new_due_str;
        self.due_cursor = self.due_str.chars().count();
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

    // Esc and Ctrl+C are intentional no-ops in edit mode: the user is required to use
    // :q / :q! / :wq to leave, so they can't bail out by mashing the wrong key.
    if matches!(key.code, KeyCode::Esc)
        || matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('c'), KeyModifiers::CONTROL)
        )
    {
        return Action::Continue;
    }

    match (key.code, key.modifiers) {
        (KeyCode::Char('z'), m) if m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::SHIFT) => {
            state.undo();
            return Action::Continue;
        }
        (KeyCode::Char('Z'), m) if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) => {
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
        Field::Due => handle_text_due(state, key),
        Field::Est => handle_text_est(state, key),
        Field::Priority => handle_priority(state, key),
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
    state.focus = if forward { state.focus.next() } else { state.focus.prev() };
    state.due_pristine = matches!(state.focus, Field::Due);
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
            if state.text_cursor > 0 {
                state.text_cursor -= 1;
            }
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

fn handle_text_due(state: &mut State, key: KeyEvent) {
    if matches!(key.code, KeyCode::Char(_)) && state.due_pristine {
        state.checkpoint();
        state.due_str.clear();
        state.due_cursor = 0;
        state.due_pristine = false;
    }
    match key.code {
        KeyCode::Char(c) => {
            state.checkpoint();
            insert_char(&mut state.due_str, &mut state.due_cursor, c);
            state.recompute_dirty();
        }
        KeyCode::Backspace => {
            state.checkpoint();
            delete_before(&mut state.due_str, &mut state.due_cursor);
            state.recompute_dirty();
        }
        KeyCode::Delete => {
            state.checkpoint();
            delete_at(&mut state.due_str, &mut state.due_cursor);
            state.recompute_dirty();
        }
        KeyCode::Left => {
            if state.due_cursor > 0 {
                state.due_cursor -= 1;
            }
        }
        KeyCode::Right => {
            let len = state.due_str.chars().count();
            if state.due_cursor < len {
                state.due_cursor += 1;
            }
        }
        KeyCode::Home => state.due_cursor = 0,
        KeyCode::End => state.due_cursor = state.due_str.chars().count(),
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
            if state.est_cursor > 0 {
                state.est_cursor -= 1;
            }
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

fn handle_priority(state: &mut State, key: KeyEvent) {
    let new_priority = match key.code {
        KeyCode::Char('a') | KeyCode::Char('A') => Some(Priority::A),
        KeyCode::Char('b') | KeyCode::Char('B') => Some(Priority::B),
        KeyCode::Char('c') | KeyCode::Char('C') => Some(Priority::C),
        KeyCode::Left => Some(match state.priority {
            Priority::A => Priority::C,
            Priority::B => Priority::A,
            Priority::C => Priority::B,
        }),
        KeyCode::Right | KeyCode::Char(' ') => Some(match state.priority {
            Priority::A => Priority::B,
            Priority::B => Priority::C,
            Priority::C => Priority::A,
        }),
        _ => None,
    };
    if let Some(p) = new_priority {
        if p != state.priority {
            state.checkpoint();
            state.priority = p;
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

    draw_field(frame, chunks[1], "Text:    ", &state.text, state.focus == Field::Text && state.mode == Mode::Edit, state.text_cursor);
    draw_priority(frame, chunks[2], state.priority, state.focus == Field::Priority && state.mode == Mode::Edit);
    draw_field(frame, chunks[3], "Due:     ", &state.due_str, state.focus == Field::Due && state.mode == Mode::Edit, state.due_cursor);
    draw_field(frame, chunks[4], "Est:     ", &state.est_str, state.focus == Field::Est && state.mode == Mode::Edit, state.est_cursor);

    let status_line: Line = if state.mode == Mode::Command {
        Line::from(Span::styled(
            format!(" :{}", state.command_buf),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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
        Paragraph::new(Span::styled(help_text, Style::default().fg(Color::DarkGray))),
        chunks[8],
    );

    if state.mode == Mode::Command {
        let x = inner.x + 2 + state.command_buf.chars().count() as u16;
        frame.set_cursor_position((x.min(inner.x + inner.width.saturating_sub(1)), chunks[6].y));
    }
}

fn draw_field(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool, cursor: usize) {
    let label_span = Span::styled(label.to_string(), Style::default().fg(Color::Cyan));
    let value_style = if focused {
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let display_width = area
        .width
        .saturating_sub(label.chars().count() as u16 + 2)
        as usize;
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

fn draw_priority(frame: &mut Frame, area: Rect, priority: Priority, focused: bool) {
    let label = Span::styled("Priority:", Style::default().fg(Color::Cyan));
    let mut spans = vec![Span::raw(" "), label, Span::raw(" ")];
    for p in [Priority::A, Priority::B, Priority::C] {
        let selected = p == priority;
        let style = if selected && focused {
            Style::default()
                .fg(Color::Black)
                .bg(priority_color(p))
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(priority_color(p)).add_modifier(Modifier::BOLD)
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

fn priority_color(p: Priority) -> Color {
    match p {
        Priority::A => Color::Red,
        Priority::B => Color::Yellow,
        Priority::C => Color::DarkGray,
    }
}

pub fn run(task: &Task, save: &mut Saver<'_>) -> Result<()> {
    // L6/L8: surface a friendly, actionable error before we try to enter raw mode.
    // Without this the crossterm calls below bubble up a bare "io error: No such
    // device" message that doesn't tell the user *what to do*.
    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::Parse(
            "the form editor requires a TTY; pass field args instead, e.g. `task edit <id> p:a` or `task edit <id> due:tomorrow`".into(),
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
        terminal
            .draw(|f| draw(f, &state))
            .map_err(Error::Io)?;

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
    use crate::model::{Priority, Status};
    use chrono::Utc;

    fn make_task() -> Task {
        Task {
            id: 1,
            text: "Buy milk".to_string(),
            priority: Priority::B,
            due: Utc::now(),
            est_secs: 1800,
            status: Status::Active,
            created_at: Utc::now(),
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
    fn from_task_seeds_due_in_relative_format() {
        let mut t = make_task();
        t.due = Utc::now() + chrono::Duration::hours(2);
        let state = State::from_task(&t);
        assert!(state.due_str.starts_with("in "));
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
        assert_eq!(state.focus, Field::Priority);
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Due);
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Est);
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Text);
    }

    #[test]
    fn enter_advances_to_next_field() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(state.focus, Field::Priority);
    }

    #[test]
    fn typing_in_text_inserts_at_cursor() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Char('!')));
        assert_eq!(state.text, "Buy milk!");
        assert_eq!(state.text_cursor, 9);
        assert!(state.dirty);
    }

    #[test]
    fn first_char_in_due_clears_existing_value() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Tab));
        handle_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, Field::Due);

        handle_key(&mut state, key(KeyCode::Char('t')));
        assert_eq!(state.due_str, "t");

        handle_key(&mut state, key(KeyCode::Char('o')));
        handle_key(&mut state, key(KeyCode::Char('m')));
        assert_eq!(state.due_str, "tom");
    }

    #[test]
    fn colon_wq_signals_save_and_quit() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        handle_key(&mut state, key(KeyCode::Char('w')));
        handle_key(&mut state, key(KeyCode::Char('q')));
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(action, Action::SaveAndQuit));
    }

    #[test]
    fn colon_w_signals_save() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        handle_key(&mut state, key(KeyCode::Char('w')));
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(action, Action::Save));
    }

    #[test]
    fn colon_q_errors_when_dirty() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Char('!')));
        handle_key(&mut state, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        handle_key(&mut state, key(KeyCode::Char('q')));
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(action, Action::Continue));
        assert!(state.error.is_some());
    }

    #[test]
    fn colon_q_when_clean_cancels() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
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
    fn rebaseline_clears_dirty_and_updates_id() {
        let mut task = make_task();
        task.id = 0;
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Char('!')));
        assert!(state.dirty);

        let mut persisted = task.clone();
        persisted.id = 17;
        persisted.text = "Buy milk!".into();
        state.rebaseline(&persisted);
        assert!(!state.dirty);
        assert_eq!(state.task_id, 17);
    }

    #[test]
    fn commit_uses_baseline_due_when_due_str_unchanged() {
        let mut task = make_task();
        task.due = Utc::now() + chrono::Duration::hours(2);
        let state = State::from_task(&task);
        let updated = state.commit(&task).unwrap();
        // Without re-parsing "in 2h", due is preserved exactly.
        assert_eq!(updated.due, task.due);
    }
}
