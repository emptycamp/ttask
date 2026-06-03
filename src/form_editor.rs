//! Built-in multi-line text editor for tasks.
//!
//! A small text area, pre-filled with the task's text. `Enter` inserts a newline so
//! a task can carry a multi-line description; `Esc` (or `Ctrl+C`) finishes and saves
//! — there is intentionally no discard key. `Ctrl+←`/`Ctrl+→` jump by word.
//!
//! For a single-line task the duration shorthand still applies: a duration token at
//! the start or end of the text (e.g. `Buy milk 45m`, `4.5h plan sprint`) is pulled
//! out into the estimate on save. A multi-line description is stored verbatim, so its
//! newlines survive (they collapse to spaces only in the compact `task ls` view).
//! Category and ord are not editable here — those are changed from the main `task`
//! view.

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
    /// Cursor position as a char index into `input`. Newlines count as one char.
    pub cursor: usize,
    pub baseline_est_secs: i64,
}

impl State {
    pub fn from_task(task: &Task) -> Self {
        Self {
            task_id: task.id,
            input: task.text.clone(),
            cursor: task.text.chars().count(),
            baseline_est_secs: task.est_secs,
        }
    }

    /// The estimate the current input would apply. The trailing/leading duration
    /// shorthand only applies to single-line input; a multi-line description is
    /// stored verbatim, so the estimate stays at the unchanged baseline.
    fn effective_est_secs(&self) -> i64 {
        match self.detected_est() {
            Some(secs) => secs,
            None => self.baseline_est_secs,
        }
    }

    /// The estimate detected from a single-line duration token, if any.
    fn detected_est(&self) -> Option<i64> {
        if self.input.contains('\n') {
            return None;
        }
        split_estimate(&self.input).1
    }

    /// Build the proposed task from `baseline`, applying the typed text. A duration
    /// token is pulled into the estimate only for single-line input; multi-line text
    /// keeps its newlines. Errors if the text would be empty.
    pub fn commit(&self, baseline: &Task) -> std::result::Result<Task, String> {
        let trimmed = self.input.trim();
        if trimmed.is_empty() {
            return Err("text cannot be empty".into());
        }
        // Multi-line text is stored verbatim (newlines preserved); only single-line
        // input feeds the `45m`-style estimate shorthand.
        let (text, est) = if trimmed.contains('\n') {
            (trimmed.to_string(), None)
        } else {
            split_estimate(trimmed)
        };
        let mut updated = baseline.clone();
        updated.text = text;
        if let Some(secs) = est {
            updated.est_secs = secs;
        }
        Ok(updated)
    }
}

pub enum Action {
    Continue,
    /// Leave the editor. The run-loop saves the committed task if it's non-empty; an
    /// empty buffer simply exits without saving (there is nothing to discard).
    Exit,
}

pub fn handle_key(state: &mut State, key: KeyEvent) -> Action {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        // Esc / Ctrl+C save and leave — there is intentionally no discard key.
        KeyCode::Esc => return Action::Exit,
        KeyCode::Char('c') if ctrl => return Action::Exit,
        // Enter inserts a newline; Esc is how you finish.
        KeyCode::Enter => insert_char(&mut state.input, &mut state.cursor, '\n'),
        KeyCode::Char(c) if !ctrl => insert_char(&mut state.input, &mut state.cursor, c),
        KeyCode::Backspace => delete_before(&mut state.input, &mut state.cursor),
        KeyCode::Delete => delete_at(&mut state.input, &mut state.cursor),
        // Ctrl+←/→ (also Alt, which some terminals send) jump by word.
        KeyCode::Left if ctrl || alt => {
            state.cursor = prev_word(&char_vec(&state.input), state.cursor)
        }
        KeyCode::Right if ctrl || alt => {
            state.cursor = next_word(&char_vec(&state.input), state.cursor)
        }
        KeyCode::Left => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Right => {
            if state.cursor < state.input.chars().count() {
                state.cursor += 1;
            }
        }
        KeyCode::Up => move_vertical(state, true),
        KeyCode::Down => move_vertical(state, false),
        KeyCode::Home => state.cursor = line_home(&char_vec(&state.input), state.cursor),
        KeyCode::End => state.cursor = line_end(&char_vec(&state.input), state.cursor),
        _ => {}
    }
    Action::Continue
}

fn char_vec(s: &str) -> Vec<char> {
    s.chars().collect()
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

/// Char index of the start of the previous word, skipping any whitespace
/// immediately to the left first. Newlines count as whitespace, so word jumps cross
/// line breaks.
fn prev_word(chars: &[char], cursor: usize) -> usize {
    let mut i = cursor.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

/// Char index of the start of the next word: skip the current run of non-whitespace,
/// then the whitespace that follows.
fn next_word(chars: &[char], cursor: usize) -> usize {
    let n = chars.len();
    let mut i = cursor.min(n);
    while i < n && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < n && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

/// Char index of the start of the logical line containing `cursor` (just after the
/// previous newline, or 0).
fn line_home(chars: &[char], cursor: usize) -> usize {
    let mut i = cursor.min(chars.len());
    while i > 0 && chars[i - 1] != '\n' {
        i -= 1;
    }
    i
}

/// Char index of the end of the logical line containing `cursor` (just before the
/// next newline, or the end of the text).
fn line_end(chars: &[char], cursor: usize) -> usize {
    let n = chars.len();
    let mut i = cursor.min(n);
    while i < n && chars[i] != '\n' {
        i += 1;
    }
    i
}

/// Move the cursor up (`up == true`) or down one logical line, keeping the column.
/// Display soft-wrapping is handled separately at render time; navigation works on
/// the newline-delimited logical lines, which keeps it independent of the terminal
/// width.
fn move_vertical(state: &mut State, up: bool) {
    let chars = char_vec(&state.input);
    let home = line_home(&chars, state.cursor);
    let col = state.cursor - home;
    if up {
        if home == 0 {
            return; // already on the first line
        }
        let prev_end = home - 1; // the newline ending the previous line
        let prev_home = line_home(&chars, prev_end);
        let prev_len = prev_end - prev_home;
        state.cursor = prev_home + col.min(prev_len);
    } else {
        let end = line_end(&chars, state.cursor);
        if end >= chars.len() {
            return; // already on the last line
        }
        let next_home = end + 1;
        let next_len = line_end(&chars, next_home) - next_home;
        state.cursor = next_home + col.min(next_len);
    }
}

/// A single visual row after soft-wrapping: the char range `[start, start + len)`.
struct VRow {
    start: usize,
    len: usize,
}

/// Lay out `chars` into visual rows for `width`, breaking on newlines and
/// soft-wrapping long logical lines. An empty logical line (and a trailing newline)
/// yields a zero-length row so the cursor can rest there.
fn visual_rows(chars: &[char], width: usize) -> Vec<VRow> {
    let width = width.max(1);
    let n = chars.len();
    let mut rows = Vec::new();
    let mut seg_start = 0;
    loop {
        let mut seg_end = seg_start;
        while seg_end < n && chars[seg_end] != '\n' {
            seg_end += 1;
        }
        let mut p = seg_start;
        loop {
            let end = (p + width).min(seg_end);
            rows.push(VRow {
                start: p,
                len: end - p,
            });
            p = end;
            if p >= seg_end {
                break;
            }
        }
        if seg_end >= n {
            break; // no trailing newline
        }
        seg_start = seg_end + 1;
        if seg_start == n {
            rows.push(VRow { start: n, len: 0 }); // trailing newline -> empty last row
            break;
        }
    }
    rows
}

/// Map a cursor char index to its `(row, col)` in the visual layout. At a soft-wrap
/// boundary the cursor belongs at the start of the next row, not the end of the one
/// that filled up.
fn cursor_rc(rows: &[VRow], cursor: usize) -> (usize, usize) {
    for (r, row) in rows.iter().enumerate() {
        let end = row.start + row.len;
        if cursor < end {
            return (r, cursor - row.start);
        }
        if cursor == end {
            if let Some(next) = rows.get(r + 1) {
                if next.start == end {
                    continue; // soft-wrap: fall through to the next row's column 0
                }
            }
            return (r, cursor - row.start);
        }
    }
    let last = rows.len().saturating_sub(1);
    (last, rows.get(last).map(|r| r.len).unwrap_or(0))
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
    let height = text_area.height.max(1) as usize;
    let chars: Vec<char> = state.input.chars().collect();
    let rows = visual_rows(&chars, width);
    let (cur_row, cur_col) = cursor_rc(&rows, state.cursor);

    // Scroll so the cursor row stays visible, pinned to the bottom once it overflows.
    let scroll = cur_row.saturating_sub(height.saturating_sub(1));
    let lines: Vec<Line> = rows
        .iter()
        .skip(scroll)
        .take(height)
        .map(|r| {
            let s: String = chars[r.start..r.start + r.len].iter().collect();
            Line::from(Span::styled(s, Style::default().fg(Color::White)))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), text_area);

    let screen_row = (cur_row - scroll) as u16;
    if screen_row < text_area.height {
        frame.set_cursor_position((text_area.x + cur_col as u16, text_area.y + screen_row));
    }

    let detected = state.detected_est();
    let est = format_est(state.effective_est_secs());
    let label = if detected.is_some() {
        format!(" estimate → {est}")
    } else {
        format!(" estimate: {est}")
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label,
            Style::default().fg(Color::Cyan),
        ))),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            " Esc save · Enter newline · Ctrl+←/→ word · a trailing duration (e.g. 45m) sets the estimate ",
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
            Action::Exit => match state.commit(&baseline) {
                Ok(updated) => {
                    save(updated)?;
                    return Ok(());
                }
                // Empty buffer: nothing valid to save, so just leave the task as it
                // was (a brand-new task is simply not created).
                Err(_) => return Ok(()),
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
    fn esc_exits() {
        let mut state = State::from_task(&make_task());
        assert!(matches!(
            handle_key(&mut state, key(KeyCode::Esc)),
            Action::Exit
        ));
    }

    #[test]
    fn ctrl_c_exits() {
        let mut state = State::from_task(&make_task());
        assert!(matches!(
            handle_key(&mut state, ctrl(KeyCode::Char('c'))),
            Action::Exit
        ));
    }

    #[test]
    fn enter_inserts_a_newline() {
        let mut state = State::from_task(&make_task());
        let action = handle_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(action, Action::Continue));
        assert_eq!(state.input, "Buy milk\n");
        assert_eq!(state.cursor, "Buy milk\n".chars().count());
    }

    #[test]
    fn ctrl_left_jumps_to_previous_word_start() {
        let mut state = State::from_task(&make_task()); // "Buy milk", cursor at 8
        handle_key(&mut state, ctrl(KeyCode::Left));
        assert_eq!(state.cursor, 4); // start of "milk"
        handle_key(&mut state, ctrl(KeyCode::Left));
        assert_eq!(state.cursor, 0); // start of "Buy"
    }

    #[test]
    fn ctrl_right_jumps_to_next_word_start() {
        let mut state = State::from_task(&make_task());
        state.cursor = 0;
        handle_key(&mut state, ctrl(KeyCode::Right));
        assert_eq!(state.cursor, 4); // past "Buy " to start of "milk"
        handle_key(&mut state, ctrl(KeyCode::Right));
        assert_eq!(state.cursor, 8); // end of text
    }

    #[test]
    fn ctrl_word_jump_crosses_newlines() {
        let mut task = make_task();
        task.text = "one\ntwo".to_string();
        let mut state = State::from_task(&task); // cursor at 7 (end)
        handle_key(&mut state, ctrl(KeyCode::Left));
        assert_eq!(state.cursor, 4); // start of "two" (after the newline)
        handle_key(&mut state, ctrl(KeyCode::Left));
        assert_eq!(state.cursor, 0); // start of "one"
    }

    #[test]
    fn up_moves_to_previous_logical_line_preserving_column() {
        let mut task = make_task();
        task.text = "abcd\nxy".to_string();
        let mut state = State::from_task(&task);
        state.cursor = 7; // col 2 on line 2
        handle_key(&mut state, key(KeyCode::Up));
        assert_eq!(state.cursor, 2); // col 2 on line 1 ("ab|cd")
    }

    #[test]
    fn down_moves_to_next_logical_line_clamping_column() {
        let mut task = make_task();
        task.text = "abcd\nxy".to_string();
        let mut state = State::from_task(&task);
        state.cursor = 4; // end of line 1 (col 4)
        handle_key(&mut state, key(KeyCode::Down));
        assert_eq!(state.cursor, 7); // clamped to end of "xy"
    }

    #[test]
    fn home_and_end_move_within_logical_line() {
        let mut task = make_task();
        task.text = "abc\ndefg".to_string();
        let mut state = State::from_task(&task);
        state.cursor = 6; // middle of "defg"
        handle_key(&mut state, key(KeyCode::Home));
        assert_eq!(state.cursor, 4); // start of line 2
        handle_key(&mut state, key(KeyCode::End));
        assert_eq!(state.cursor, 8); // end of line 2
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
    fn commit_preserves_newlines_and_skips_estimate_shorthand() {
        let task = make_task();
        let mut state = State::from_task(&task);
        handle_key(&mut state, key(KeyCode::Enter));
        for c in "extra 30m".chars() {
            handle_key(&mut state, key(KeyCode::Char(c)));
        }
        let updated = state.commit(&task).unwrap();
        // Newlines survive, and the trailing `30m` is NOT treated as an estimate
        // because the text is multi-line.
        assert_eq!(updated.text, "Buy milk\nextra 30m");
        assert_eq!(updated.est_secs, 1800);
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
    fn visual_rows_breaks_on_newline_and_wraps() {
        let chars: Vec<char> = "abcd\nef".chars().collect();
        let rows = visual_rows(&chars, 2);
        let texts: Vec<String> = rows
            .iter()
            .map(|r| chars[r.start..r.start + r.len].iter().collect())
            .collect();
        assert_eq!(texts, vec!["ab", "cd", "ef"]);
    }

    #[test]
    fn cursor_rc_defers_soft_wrap_boundary_to_next_row() {
        let chars: Vec<char> = "abcd".chars().collect();
        let rows = visual_rows(&chars, 2); // [{0,2},{2,2}]
        assert_eq!(cursor_rc(&rows, 0), (0, 0));
        assert_eq!(cursor_rc(&rows, 2), (1, 0)); // boundary -> next row start
        assert_eq!(cursor_rc(&rows, 4), (1, 2)); // end of text
    }
}
