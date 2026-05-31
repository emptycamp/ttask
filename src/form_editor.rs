//! Built-in single-field text editor for tasks.
//!
//! One text input, pre-filled with the task's text and wrapped across lines when
//! long. The user only ever edits the text. To change the estimate they type a
//! duration token at the start or end of the text (e.g. `Buy milk 45m`, `4.5h plan
//! sprint`) — exactly the shorthand `task add` accepts. On Enter the token is
//! pulled out and applied to the estimate without polluting the text; Esc discards
//! everything. Category and ord are not editable here — those are changed from the
//! main `task` view.

use crate::editor::Saver;
use crate::error::{Error, Result};
use crate::format::format_est;
use crate::model::Task;
use crate::time::parse_fields::split_estimate;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use std::io;

#[derive(Clone, Debug)]
pub struct State {
    pub task_id: u32,
    pub input: String,
    /// Cursor position as a char index into `input`.
    pub cursor: usize,
    pub baseline_est_secs: i64,
    pub error: Option<String>,
}

impl State {
    pub fn from_task(task: &Task) -> Self {
        Self {
            task_id: task.id,
            input: task.text.clone(),
            cursor: task.text.chars().count(),
            baseline_est_secs: task.est_secs,
            error: None,
        }
    }

    /// The estimate the current input would apply: a detected token, otherwise the
    /// unchanged baseline.
    fn effective_est_secs(&self) -> i64 {
        match split_estimate(&self.input) {
            (_, Some(secs)) => secs,
            (_, None) => self.baseline_est_secs,
        }
    }

    /// Build the proposed task from `baseline`, applying the typed text and any
    /// duration token found in it. Errors if the text would be empty.
    pub fn commit(&self, baseline: &Task) -> std::result::Result<Task, String> {
        let (text, est) = split_estimate(&self.input);
        let text = text.trim();
        if text.is_empty() {
            return Err("text cannot be empty".into());
        }
        let mut updated = baseline.clone();
        updated.text = text.to_string();
        if let Some(secs) = est {
            updated.est_secs = secs;
        }
        Ok(updated)
    }
}

pub enum Action {
    Continue,
    Confirm,
    Cancel,
}

pub fn handle_key(state: &mut State, key: KeyEvent) -> Action {
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Action::Cancel,
        (KeyCode::Enter, _) => return Action::Confirm,
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            insert_char(&mut state.input, &mut state.cursor, c);
            state.error = None;
        }
        (KeyCode::Backspace, _) => {
            delete_before(&mut state.input, &mut state.cursor);
            state.error = None;
        }
        (KeyCode::Delete, _) => {
            delete_at(&mut state.input, &mut state.cursor);
            state.error = None;
        }
        (KeyCode::Left, _) => state.cursor = state.cursor.saturating_sub(1),
        (KeyCode::Right, _) => {
            let len = state.input.chars().count();
            if state.cursor < len {
                state.cursor += 1;
            }
        }
        (KeyCode::Home, _) => state.cursor = 0,
        (KeyCode::End, _) => state.cursor = state.input.chars().count(),
        _ => {}
    }
    Action::Continue
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

/// Hard-wrap `chars` into lines of at most `width` chars. Character wrapping (not
/// word wrapping) keeps cursor math trivial: char index `i` is always at row
/// `i / width`, column `i % width`.
fn wrap_chars(chars: &[char], width: usize) -> Vec<String> {
    if width == 0 {
        return vec![chars.iter().collect()];
    }
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars
        .chunks(width)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn draw(frame: &mut Frame, state: &State) {
    let area = frame.area();
    let title = if state.task_id == 0 {
        " New Task ".to_string()
    } else {
        format!(" Edit Task #{} ", state.task_id)
    };
    let outer = Block::default().borders(Borders::ALL).title(title);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // wrapped text input
            Constraint::Length(1), // estimate hint
            Constraint::Length(1), // help
        ])
        .split(inner);

    let text_area = chunks[0];
    let width = text_area.width.max(1) as usize;
    let chars: Vec<char> = state.input.chars().collect();
    let lines: Vec<Line> = wrap_chars(&chars, width)
        .into_iter()
        .map(|l| Line::from(Span::styled(l, Style::default().fg(Color::White))))
        .collect();
    frame.render_widget(Paragraph::new(lines), text_area);

    // Cursor row/col follow directly from the char index because we hard-wrap.
    let cur_row = (state.cursor / width) as u16;
    let cur_col = (state.cursor % width) as u16;
    if cur_row < text_area.height {
        frame.set_cursor_position((text_area.x + cur_col, text_area.y + cur_row));
    }

    let hint: Line = if let Some(err) = &state.error {
        Line::from(Span::styled(
            format!(" ! {err}"),
            Style::default().fg(Color::Red),
        ))
    } else {
        let (_, detected) = split_estimate(&state.input);
        let est = format_est(state.effective_est_secs());
        let label = if detected.is_some() {
            format!(" estimate → {est}")
        } else {
            format!(" estimate: {est}")
        };
        Line::from(Span::styled(label, Style::default().fg(Color::Cyan)))
    };
    frame.render_widget(Paragraph::new(hint), chunks[1]);

    frame.render_widget(
        Paragraph::new(Span::styled(
            " Enter save · Esc cancel · append a duration (e.g. 45m) to set the estimate ",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[2],
    );
}

pub fn run(task: &Task, save: &mut Saver<'_>) -> Result<()> {
    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::Parse(
            "the editor requires a TTY; pass field args instead, e.g. `task edit <id> c:a` or `task edit <id> ord:1`".into(),
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
    let baseline = initial.clone();
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
            Action::Confirm => match state.commit(&baseline) {
                Ok(updated) => {
                    save(updated)?;
                    return Ok(());
                }
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

    #[test]
    fn from_task_prefills_text_and_cursor_at_end() {
        let state = State::from_task(&make_task());
        assert_eq!(state.input, "Buy milk");
        assert_eq!(state.cursor, "Buy milk".chars().count());
    }

    #[test]
    fn typing_inserts_at_cursor() {
        let mut state = State::from_task(&make_task());
        handle_key(&mut state, key(KeyCode::Char('!')));
        assert_eq!(state.input, "Buy milk!");
    }

    #[test]
    fn backspace_deletes_before_cursor() {
        let mut state = State::from_task(&make_task());
        handle_key(&mut state, key(KeyCode::Backspace));
        assert_eq!(state.input, "Buy mil");
    }

    #[test]
    fn esc_cancels() {
        let mut state = State::from_task(&make_task());
        assert!(matches!(
            handle_key(&mut state, key(KeyCode::Esc)),
            Action::Cancel
        ));
    }

    #[test]
    fn enter_confirms() {
        let mut state = State::from_task(&make_task());
        assert!(matches!(
            handle_key(&mut state, key(KeyCode::Enter)),
            Action::Confirm
        ));
    }

    #[test]
    fn commit_leaves_estimate_when_no_duration_typed() {
        let task = make_task();
        let state = State::from_task(&task);
        let updated = state.commit(&task).unwrap();
        assert_eq!(updated.text, "Buy milk");
        assert_eq!(updated.est_secs, 1800);
    }

    #[test]
    fn commit_extracts_trailing_duration_into_estimate() {
        let task = make_task();
        let mut state = State::from_task(&task);
        for c in " 45m".chars() {
            handle_key(&mut state, key(KeyCode::Char(c)));
        }
        let updated = state.commit(&task).unwrap();
        assert_eq!(updated.text, "Buy milk");
        assert_eq!(updated.est_secs, 45 * 60);
    }

    #[test]
    fn commit_extracts_leading_fractional_hours() {
        let mut task = make_task();
        task.text = String::new();
        let mut state = State::from_task(&task);
        for c in "4.5h plan sprint".chars() {
            handle_key(&mut state, key(KeyCode::Char(c)));
        }
        let updated = state.commit(&task).unwrap();
        assert_eq!(updated.text, "plan sprint");
        assert_eq!(updated.est_secs, 4 * 3600 + 1800);
    }

    #[test]
    fn commit_rejects_empty_text() {
        let mut task = make_task();
        task.text = String::new();
        let state = State::from_task(&task);
        assert!(state.commit(&task).is_err());
    }

    #[test]
    fn commit_does_not_change_category_or_ord() {
        let mut task = make_task();
        task.category = Category::A;
        task.ord = 7;
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Char('X')));
        let updated = state.commit(&task).unwrap();
        assert_eq!(updated.category, Category::A);
        assert_eq!(updated.ord, 7);
    }

    #[test]
    fn wrap_chars_splits_on_width() {
        let chars: Vec<char> = "abcdef".chars().collect();
        assert_eq!(wrap_chars(&chars, 2), vec!["ab", "cd", "ef"]);
    }
}
