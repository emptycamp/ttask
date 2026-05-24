use crate::format::effective_day;
use crate::model::{Priority, TaskId};
use crate::tui::App;
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum PendingChange {
    ToggleComplete(TaskId),
    ToggleDelete(TaskId),
    SetPriority(TaskId, Priority),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Continue,
    Quit,
    EditTask(TaskId),
    AddTask,
}

pub fn handle(app: &mut App, key: KeyEvent) -> Action {
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) => {
            app.cursor = app.cursor.saturating_sub(1);
        }
        (KeyCode::Down, _) => {
            if app.cursor + 1 < app.tasks.len() {
                app.cursor += 1;
            }
        }
        (KeyCode::Left, _) => {
            if let Some(idx) = prev_day_first_task(&app.tasks, app.cursor) {
                app.cursor = idx;
            }
        }
        (KeyCode::Right, _) => {
            if let Some(idx) = next_day_first_task(&app.tasks, app.cursor) {
                app.cursor = idx;
            }
        }
        (KeyCode::Char('a'), KeyModifiers::NONE) => {
            return Action::AddTask;
        }
        (KeyCode::Char('c'), KeyModifiers::NONE) => {
            if let Some(task) = app.tasks.get(app.cursor) {
                toggle_change(app, PendingChange::ToggleComplete(task.id));
            }
        }
        (KeyCode::Char('d'), KeyModifiers::NONE) => {
            if let Some(task) = app.tasks.get(app.cursor) {
                toggle_change(app, PendingChange::ToggleDelete(task.id));
            }
        }
        (KeyCode::Char('e'), KeyModifiers::NONE) => {
            if let Some(task) = app.tasks.get(app.cursor) {
                return Action::EditTask(task.id);
            }
        }
        (KeyCode::Char('A'), KeyModifiers::SHIFT) => {
            if let Some(task) = app.tasks.get(app.cursor) {
                set_priority(app, task.id, Priority::A);
            }
        }
        (KeyCode::Char('B'), KeyModifiers::SHIFT) => {
            if let Some(task) = app.tasks.get(app.cursor) {
                set_priority(app, task.id, Priority::B);
            }
        }
        (KeyCode::Char('C'), KeyModifiers::SHIFT) => {
            if let Some(task) = app.tasks.get(app.cursor) {
                set_priority(app, task.id, Priority::C);
            }
        }
        (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Action::Quit;
        }
        _ => {}
    }
    Action::Continue
}

/// Find the index of the first task on the day immediately preceding `tasks[cursor]`.
/// Uses `effective_day` so overdue active tasks share the "Today" group.
fn prev_day_first_task(tasks: &[crate::model::Task], cursor: usize) -> Option<usize> {
    if cursor == 0 || tasks.is_empty() {
        return None;
    }
    let today = Local::now().date_naive();
    let current_day = effective_day(&tasks[cursor], today);
    let mut i = cursor;
    while i > 0 {
        i -= 1;
        if effective_day(&tasks[i], today) != current_day {
            let prev_day = effective_day(&tasks[i], today);
            while i > 0 && effective_day(&tasks[i - 1], today) == prev_day {
                i -= 1;
            }
            return Some(i);
        }
    }
    None
}

/// Find the index of the first task on the day immediately following `tasks[cursor]`.
fn next_day_first_task(tasks: &[crate::model::Task], cursor: usize) -> Option<usize> {
    if tasks.is_empty() {
        return None;
    }
    let today = Local::now().date_naive();
    let current_day = effective_day(&tasks[cursor], today);
    for (i, t) in tasks.iter().enumerate().skip(cursor + 1) {
        if effective_day(t, today) != current_day {
            return Some(i);
        }
    }
    None
}

fn toggle_change(app: &mut App, change: PendingChange) {
    let id = match &change {
        PendingChange::ToggleComplete(id) => *id,
        PendingChange::ToggleDelete(id) => *id,
        PendingChange::SetPriority(id, _) => *id,
    };
    let changes = app.pending.entry(id).or_default();
    if let Some(pos) = changes.iter().position(|c| c == &change) {
        changes.remove(pos);
    } else {
        changes.push(change);
    }
}

fn set_priority(app: &mut App, id: TaskId, p: Priority) {
    let changes = app.pending.entry(id).or_default();
    changes.retain(|c| !matches!(c, PendingChange::SetPriority(_, _)));
    changes.push(PendingChange::SetPriority(id, p));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Priority, Status, Task};
    use crate::tui::App;
    use chrono::{Duration, Utc};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_task_due(id: u32, due: chrono::DateTime<Utc>) -> Task {
        Task {
            id,
            text: format!("task {id}"),
            priority: Priority::B,
            due,
            est_secs: 1800,
            status: Status::Active,
            created_at: Utc::now(),
            completed_at: None,
            deleted_at: None,
        }
    }

    fn make_task(id: u32) -> Task {
        make_task_due(id, Utc::now())
    }

    fn make_app() -> App {
        App {
            tasks: vec![make_task(1), make_task(2)],
            cursor: 0,
            pending: std::collections::HashMap::new(),
            should_quit: false,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    #[test]
    fn up_moves_cursor_up_clamped() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Up));
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn down_moves_cursor_down() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Down));
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn down_clamped_at_last_item() {
        let mut app = make_app();
        app.cursor = 1;
        handle(&mut app, key(KeyCode::Down));
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn left_jumps_to_first_task_of_previous_day() {
        let now = Utc::now();
        let mut app = App {
            tasks: vec![
                make_task_due(1, now),
                make_task_due(2, now),
                make_task_due(3, now + Duration::days(1)),
                make_task_due(4, now + Duration::days(1)),
            ],
            cursor: 3,
            pending: std::collections::HashMap::new(),
            should_quit: false,
        };
        handle(&mut app, key(KeyCode::Left));
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn left_at_first_day_is_noop() {
        let now = Utc::now();
        let mut app = App {
            tasks: vec![make_task_due(1, now), make_task_due(2, now)],
            cursor: 1,
            pending: std::collections::HashMap::new(),
            should_quit: false,
        };
        handle(&mut app, key(KeyCode::Left));
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn right_jumps_to_first_task_of_next_day() {
        let now = Utc::now();
        let mut app = App {
            tasks: vec![
                make_task_due(1, now),
                make_task_due(2, now),
                make_task_due(3, now + Duration::days(1)),
                make_task_due(4, now + Duration::days(1)),
            ],
            cursor: 0,
            pending: std::collections::HashMap::new(),
            should_quit: false,
        };
        handle(&mut app, key(KeyCode::Right));
        assert_eq!(app.cursor, 2);
    }

    #[test]
    fn a_returns_add_action() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('a')));
        assert_eq!(action, Action::AddTask);
    }

    #[test]
    fn c_key_adds_toggle_complete_pending() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Char('c')));
        let changes = app.pending.get(&1).unwrap();
        assert!(changes.contains(&PendingChange::ToggleComplete(1)));
    }

    #[test]
    fn d_key_adds_toggle_delete_pending() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Char('d')));
        let changes = app.pending.get(&1).unwrap();
        assert!(changes.contains(&PendingChange::ToggleDelete(1)));
    }

    #[test]
    fn e_key_returns_edit_action() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('e')));
        assert_eq!(action, Action::EditTask(1));
    }

    #[test]
    fn enter_is_noop() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Enter));
        assert_eq!(action, Action::Continue);
    }

    #[test]
    fn shift_a_sets_priority_a() {
        let mut app = make_app();
        handle(&mut app, shift_key(KeyCode::Char('A')));
        let changes = app.pending.get(&1).unwrap();
        assert!(changes.contains(&PendingChange::SetPriority(1, Priority::A)));
    }

    #[test]
    fn esc_signals_quit() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Esc));
        assert_eq!(action, Action::Quit);
    }

    #[test]
    fn ctrl_c_signals_quit() {
        let mut app = make_app();
        let action = handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, Action::Quit);
    }
}
