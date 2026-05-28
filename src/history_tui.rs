//! Interactive picker for `task history`.
//!
//! Behaves like `task` view's pending-change model:
//! - `u` or Enter toggles a "mark for undo" anchor on the selected event.
//! - Marks live per-task: anchoring on a task #1 event marks task #1's cascade; you
//!   can independently anchor on a task #2 event to add task #2's cascade. Marking
//!   never reaches across tasks (separate tasks aren't connected).
//! - Within each task, marking an event also marks every newer event on the same
//!   task. The marked events are shown struck-through in red.
//! - Esc / Ctrl+C exits and applies all marks (newest first across the union).
//!   `q` does nothing, matching `task` view's keymap.

use crate::error::{Error, Result};
use crate::format::format_relative_past;
use crate::model::TaskId;
use crate::store::revert::HistoryEntry;
use crate::store::Store;
use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use std::collections::HashMap;
use std::io;

pub struct App {
    /// Events sorted newest-first.
    pub entries: Vec<(u64, HistoryEntry)>,
    pub cursor: usize,
    /// One anchor per task — the oldest event on that task that's marked for undo.
    /// Each anchor implies "this event and every newer event on the same task".
    pub anchors: HashMap<TaskId, u64>,
    pub error: Option<String>,
}

impl App {
    pub fn from_entries(entries: Vec<(u64, HistoryEntry)>) -> Self {
        Self {
            entries,
            cursor: 0,
            anchors: HashMap::new(),
            error: None,
        }
    }

    /// Task id of an arbitrary event id, by looking it up in entries.
    fn task_of(&self, event_id: u64) -> Option<TaskId> {
        self.entries
            .iter()
            .find(|(id, _)| *id == event_id)
            .map(|(_, e)| e.op.task_id())
    }

    pub fn is_marked(&self, event_id: u64) -> bool {
        let Some(task_id) = self.task_of(event_id) else {
            return false;
        };
        let Some(&anchor) = self.anchors.get(&task_id) else {
            return false;
        };
        event_id >= anchor
    }

    /// Toggle the mark anchor at the cursor's event. Affects only the event's task —
    /// other tasks' anchors are left alone. Pressing on a task's current anchor
    /// clears that task's marks; pressing on any other event sets/moves that task's
    /// anchor.
    pub fn toggle_mark_at_cursor(&mut self) {
        let Some((event_id, entry)) = self.entries.get(self.cursor) else {
            return;
        };
        let event_id = *event_id;
        let task_id = entry.op.task_id();

        match self.anchors.get(&task_id).copied() {
            Some(existing) if existing == event_id => {
                self.anchors.remove(&task_id);
            }
            _ => {
                self.anchors.insert(task_id, event_id);
            }
        }
        self.error = None;
    }

    /// Return ids to revert in apply order (newest first across all tasks). Each
    /// per-task cascade is independent, so global newest-first ordering is safe:
    /// reverting an event on task A doesn't affect task B's state.
    pub fn cascade_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .entries
            .iter()
            .filter(|(id, _)| self.is_marked(*id))
            .map(|(id, _)| *id)
            .collect();
        ids.sort_by(|a, b| b.cmp(a));
        ids
    }

    /// Number of distinct tasks currently anchored.
    pub fn marked_task_count(&self) -> usize {
        self.anchors.len()
    }
}

fn load_entries(store: &Store) -> Result<Vec<(u64, HistoryEntry)>> {
    let mut entries = store.history()?;
    entries.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    Ok(entries)
}

pub fn run(store: &mut Store) -> Result<()> {
    let mut app = App::from_entries(load_entries(store)?);

    enable_raw_mode().map_err(Error::Io)?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(Error::Io)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(Error::Io)?;

    let result = run_loop(&mut terminal, &mut app);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result?;

    // Apply all marked reverts, newest first. Errors bubble up so the user sees them
    // rather than silently leaving half-state.
    for id in app.cascade_ids() {
        store.history_revert(id)?;
    }
    Ok(())
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| draw(f, app)).map_err(Error::Io)?;

        let key = match event::read().map_err(Error::Io)? {
            Event::Key(k) if k.kind == KeyEventKind::Press => k,
            _ => continue,
        };

        match (key.code, key.modifiers) {
            // Quit keys match `task` view exactly. `q` deliberately does nothing.
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                return Ok(());
            }
            (KeyCode::Up, _) => {
                app.cursor = app.cursor.saturating_sub(1);
            }
            (KeyCode::Down, _) => {
                app.cursor = (app.cursor + 1).min(app.entries.len().saturating_sub(1));
            }
            (KeyCode::Char('u'), KeyModifiers::NONE) | (KeyCode::Enter, _) => {
                app.toggle_mark_at_cursor();
            }
            _ => {}
        }
    }
}

fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(3),    // list
            Constraint::Length(1), // status
            Constraint::Length(1), // help
        ])
        .split(frame.area());

    let header = Paragraph::new(Span::styled(
        "   ID  When         Event",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(header, chunks[0]);

    let row_width = chunks[1].width.saturating_sub(2) as usize;
    let now = Utc::now();

    let items: Vec<ListItem> = if app.entries.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No history.",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        app.entries
            .iter()
            .map(|(id, entry)| {
                let marked = app.is_marked(*id);
                let prefix = if marked { "✗ " } else { "  " };
                let when = format_relative_past(entry.timestamp, now);
                let summary = entry.op.summary();
                let mut text = format!("{}{:>4}  {:<11}  {}", prefix, id, when, summary);
                while text.chars().count() < row_width {
                    text.push(' ');
                }
                let style = if marked {
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(Span::styled(text, style)))
            })
            .collect()
    };

    let mut state = ListState::default();
    if !app.entries.is_empty() {
        state.select(Some(app.cursor));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    ratatui::widgets::StatefulWidget::render(list, chunks[1], frame.buffer_mut(), &mut state);

    let status_line: Line = if let Some(err) = &app.error {
        Line::from(Span::styled(
            format!(" ! {err}"),
            Style::default().fg(Color::Red),
        ))
    } else if !app.anchors.is_empty() {
        let count = app.cascade_ids().len();
        let task_count = app.marked_task_count();
        let msg = if task_count == 1 {
            // Sole anchor — name the task to make the scope concrete.
            let (task_id, anchor) = app.anchors.iter().next().unwrap();
            if count == 1 {
                format!(" 1 event marked (#{anchor}) on task #{task_id}")
            } else {
                format!(" {count} events marked on task #{task_id} — cascade from #{anchor}")
            }
        } else {
            format!(" {count} events marked across {task_count} tasks")
        };
        Line::from(Span::styled(msg, Style::default().fg(Color::Yellow)))
    } else {
        Line::from(Span::raw(""))
    };
    frame.render_widget(Paragraph::new(status_line), chunks[2]);

    let help = Paragraph::new(Span::styled(
        " ↑↓ navigate · u/Enter toggle undo · Esc apply & quit ",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(help, chunks[3]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::revert::RevertOp;
    use chrono::Utc;

    /// Build N events that all touch the same task — that's what the cascade is
    /// scoped to, so this is the relevant shape for the navigation tests.
    fn same_task_entries(event_ids: &[u64], task_id: u32) -> Vec<(u64, HistoryEntry)> {
        use crate::model::{Category, Status, Task};
        let now = Utc::now();
        let baseline = Task {
            id: task_id,
            text: format!("task {task_id}"),
            category: Category::B,
            ord: task_id,
            est_secs: 0,
            status: Status::Active,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        };
        event_ids
            .iter()
            .rev() // newest first
            .enumerate()
            .map(|(i, id)| {
                let op = if i == event_ids.len() - 1 {
                    // The oldest entry is the original add.
                    RevertOp::Added {
                        task: baseline.clone(),
                    }
                } else {
                    RevertOp::Edited {
                        before: baseline.clone(),
                        after: baseline.clone(),
                    }
                };
                (*id, HistoryEntry { op, timestamp: now })
            })
            .collect()
    }

    #[test]
    fn empty_cascade_when_no_anchor() {
        let app = App::from_entries(same_task_entries(&[1, 2, 3], 1));
        assert!(app.cascade_ids().is_empty());
    }

    #[test]
    fn marking_cursor_event_sets_anchor() {
        let mut app = App::from_entries(same_task_entries(&[1, 2, 3], 1));
        // Newest-first: entries[0] = (3, ...), entries[1] = (2, ...), entries[2] = (1, ...).
        app.cursor = 1; // event id 2
        app.toggle_mark_at_cursor();
        assert_eq!(app.anchors.get(&1), Some(&2));
    }

    #[test]
    fn cascade_includes_target_and_all_newer_within_same_task() {
        let mut app = App::from_entries(same_task_entries(&[1, 2, 3, 4, 5], 1));
        // Move cursor to event id 3 (the middle).
        app.cursor = 2;
        app.toggle_mark_at_cursor();
        let cascade = app.cascade_ids();
        assert_eq!(cascade, vec![5, 4, 3]);
    }

    #[test]
    fn marking_older_extends_cascade_within_same_task() {
        let mut app = App::from_entries(same_task_entries(&[1, 2, 3], 1));
        app.cursor = 0; // id 3
        app.toggle_mark_at_cursor();
        assert_eq!(app.cascade_ids(), vec![3]);
        // Move down to older id and re-anchor.
        app.cursor = 2; // id 1
        app.toggle_mark_at_cursor();
        assert_eq!(app.cascade_ids(), vec![3, 2, 1]);
    }

    #[test]
    fn pressing_on_current_anchor_clears_marks() {
        let mut app = App::from_entries(same_task_entries(&[1, 2, 3], 1));
        app.cursor = 1; // id 2
        app.toggle_mark_at_cursor();
        assert_eq!(app.anchors.get(&1), Some(&2));
        app.toggle_mark_at_cursor();
        assert!(app.anchors.is_empty());
        assert!(app.cascade_ids().is_empty());
    }

    #[test]
    fn is_marked_checks_cascade_membership() {
        let mut app = App::from_entries(same_task_entries(&[1, 2, 3], 1));
        app.cursor = 1; // id 2
        app.toggle_mark_at_cursor();
        assert!(app.is_marked(3));
        assert!(app.is_marked(2));
        assert!(!app.is_marked(1));
    }

    /// Multiple anchors: a user can mark events from several tasks and the per-task
    /// cascades all show up together.
    #[test]
    fn anchors_on_two_tasks_union_their_cascades() {
        use crate::model::{Category, Status, Task};
        let now = Utc::now();
        fn t(id: u32) -> Task {
            let now = Utc::now();
            Task {
                id,
                text: format!("task {id}"),
                category: Category::B,
                ord: id,
                est_secs: 0,
                status: Status::Active,
                created_at: now,
                updated_at: now,
                completed_at: None,
                deleted_at: None,
            }
        }
        // Newest-first history:
        //   5 → edited #2
        //   4 → edited #1
        //   3 → added #2
        //   2 → edited #1
        //   1 → added #1
        let entries = vec![
            (
                5,
                HistoryEntry {
                    op: RevertOp::Edited {
                        before: t(2),
                        after: t(2),
                    },
                    timestamp: now,
                },
            ),
            (
                4,
                HistoryEntry {
                    op: RevertOp::Edited {
                        before: t(1),
                        after: t(1),
                    },
                    timestamp: now,
                },
            ),
            (
                3,
                HistoryEntry {
                    op: RevertOp::Added { task: t(2) },
                    timestamp: now,
                },
            ),
            (
                2,
                HistoryEntry {
                    op: RevertOp::Edited {
                        before: t(1),
                        after: t(1),
                    },
                    timestamp: now,
                },
            ),
            (
                1,
                HistoryEntry {
                    op: RevertOp::Added { task: t(1) },
                    timestamp: now,
                },
            ),
        ];
        let mut app = App::from_entries(entries);

        // Anchor on event 1 (task #1 added) — that drags in events 2 & 4 (task #1's
        // edits) but not events 3 & 5 (task #2).
        app.cursor = 4; // event id 1
        app.toggle_mark_at_cursor();
        assert_eq!(app.cascade_ids(), vec![4, 2, 1]);

        // Anchor on event 3 (task #2 added) — now task #2's cascade joins.
        app.cursor = 2; // event id 3
        app.toggle_mark_at_cursor();
        assert_eq!(app.marked_task_count(), 2);
        assert_eq!(app.cascade_ids(), vec![5, 4, 3, 2, 1]);

        // Clear task #1's anchor by toggling it again; task #2 remains anchored.
        app.cursor = 4;
        app.toggle_mark_at_cursor();
        assert_eq!(app.marked_task_count(), 1);
        assert_eq!(app.cascade_ids(), vec![5, 3]);
    }

    /// Mixed-task entries: anchoring on a #task-A event must NOT mark newer #task-B events.
    #[test]
    fn cascade_skips_events_for_other_tasks() {
        use crate::model::{Category, Status, Task};
        let now = Utc::now();
        fn t(id: u32) -> Task {
            let now = Utc::now();
            Task {
                id,
                text: format!("task {id}"),
                category: Category::B,
                ord: id,
                est_secs: 0,
                status: Status::Active,
                created_at: now,
                updated_at: now,
                completed_at: None,
                deleted_at: None,
            }
        }
        // History (newest-first): event 4 edits task #1, event 3 adds task #2,
        // event 2 edits task #1, event 1 adds task #1.
        let entries = vec![
            (
                4,
                HistoryEntry {
                    op: RevertOp::Edited {
                        before: t(1),
                        after: t(1),
                    },
                    timestamp: now,
                },
            ),
            (
                3,
                HistoryEntry {
                    op: RevertOp::Added { task: t(2) },
                    timestamp: now,
                },
            ),
            (
                2,
                HistoryEntry {
                    op: RevertOp::Edited {
                        before: t(1),
                        after: t(1),
                    },
                    timestamp: now,
                },
            ),
            (
                1,
                HistoryEntry {
                    op: RevertOp::Added { task: t(1) },
                    timestamp: now,
                },
            ),
        ];
        let mut app = App::from_entries(entries);
        // Anchor on event 1 (add task #1).
        app.cursor = 3;
        app.toggle_mark_at_cursor();
        // Cascade should contain task #1's events (4, 2, 1), skipping event 3 (#2).
        assert_eq!(app.cascade_ids(), vec![4, 2, 1]);
        assert!(app.is_marked(4));
        assert!(app.is_marked(2));
        assert!(app.is_marked(1));
        assert!(
            !app.is_marked(3),
            "task #2 event should not be in the cascade"
        );
    }
}
