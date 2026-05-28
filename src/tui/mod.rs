pub mod events;
pub mod pending;
pub mod render;

use crate::clock::Clock;
use crate::editor::TaskEditor;
use crate::error::{Error, Result};
use crate::format::sort_key;
use crate::model::{Category, Status, Task, TaskId};
use crate::store::Store;
use crate::tui::events::{Action, PendingChange};
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::collections::HashMap;
use std::io;
use std::time::{Duration as StdDuration, Instant};

const EVENT_POLL_MS: u64 = 100;
/// How often to re-read the store so external `task add/edit/delete/complete`
/// invocations show up live in the open TUI. 500 ms balances "feels live" with
/// "don't spam the store with read txns".
const EXTERNAL_REFRESH_MS: u64 = 500;

pub struct App {
    pub tasks: Vec<Task>,
    pub cursor: usize,
    pub pending: HashMap<TaskId, Vec<PendingChange>>,
    pub should_quit: bool,
    /// `Some(buf)` while the user is editing the search prompt; `None` when not in
    /// search-input mode. While editing, the in-progress buffer is also the live
    /// filter applied to the displayed list.
    pub search_input: Option<String>,
    /// The committed filter — survives across input/exit transitions. Empty means
    /// no filter is applied. Matches case-insensitively against task text.
    pub search_filter: String,
}

impl App {
    pub fn new(tasks: Vec<Task>) -> Self {
        Self {
            tasks,
            cursor: 0,
            pending: HashMap::new(),
            should_quit: false,
            search_input: None,
            search_filter: String::new(),
        }
    }

    pub fn effective_filter(&self) -> &str {
        self.search_input
            .as_deref()
            .unwrap_or(self.search_filter.as_str())
    }

    pub fn filtered_tasks(&self) -> Vec<&Task> {
        let f = self.effective_filter().trim().to_lowercase();
        if f.is_empty() {
            return self.tasks.iter().collect();
        }
        self.tasks
            .iter()
            .filter(|t| t.text.to_lowercase().contains(&f))
            .collect()
    }

    pub fn cursor_task(&self) -> Option<&Task> {
        self.filtered_tasks().get(self.cursor).copied()
    }

    pub fn clamp_cursor(&mut self) {
        let len = self.filtered_tasks().len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }
}

pub fn run(store: &mut Store, clock: &dyn Clock, editor: &dyn TaskEditor) -> Result<()> {
    let mut app = App::new(load_active_tasks(store)?);

    enter_screen()?;
    let mut terminal = build_terminal()?;

    let result = run_loop(&mut terminal, &mut app, store, clock, editor);

    leave_screen(&mut terminal);

    result?;

    pending::apply(&app.pending, store, clock)?;
    Ok(())
}

fn load_active_tasks(store: &Store) -> Result<Vec<Task>> {
    let mut tasks: Vec<Task> = store
        .all_tasks()?
        .into_iter()
        .filter(|t| t.status == Status::Active)
        .collect();
    tasks.sort_by_key(sort_key);
    Ok(tasks)
}

fn build_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    let backend = CrosstermBackend::new(io::stdout());
    Terminal::new(backend).map_err(Error::Io)
}

fn enter_screen() -> Result<()> {
    enable_raw_mode().map_err(Error::Io)?;
    execute!(io::stdout(), EnterAlternateScreen).map_err(Error::Io)?;
    Ok(())
}

fn leave_screen(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<()> {
    let mut last_external_refresh = Instant::now();
    loop {
        if app.search_input.is_none()
            && last_external_refresh.elapsed().as_millis() >= EXTERNAL_REFRESH_MS as u128
        {
            refresh_tasks(app, store)?;
            last_external_refresh = Instant::now();
        }

        terminal.draw(|f| render::draw(f, app)).map_err(Error::Io)?;

        if !event::poll(StdDuration::from_millis(EVENT_POLL_MS)).map_err(Error::Io)? {
            continue;
        }
        let key = match event::read().map_err(Error::Io)? {
            Event::Key(k) if k.kind == KeyEventKind::Press => k,
            _ => continue,
        };

        match events::handle(app, key) {
            Action::Continue => {}
            Action::Quit => return Ok(()),
            Action::EditTask(id) => {
                let edit_result =
                    with_paused_terminal(terminal, || edit_existing(id, store, clock, editor));
                edit_result?;
                refresh_tasks(app, store)?;
            }
            Action::AddTask => {
                let add_result = with_paused_terminal(terminal, || add_new(store, clock, editor));
                add_result?;
                refresh_tasks(app, store)?;
            }
            Action::ReorderCursor(target_ord) => {
                if let Some(id) = app.cursor_task().map(|t| t.id) {
                    store.reorder_task(id, target_ord, clock)?;
                    refresh_tasks(app, store)?;
                    // Keep the cursor on the moved task at its new position.
                    let pos = app.filtered_tasks().iter().position(|t| t.id == id);
                    if let Some(p) = pos {
                        app.cursor = p;
                    }
                }
            }
        }
    }
}

fn with_paused_terminal<F, T>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    f: F,
) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    let result = f();

    let _ = enable_raw_mode();
    let _ = execute!(terminal.backend_mut(), EnterAlternateScreen);
    let _ = terminal.clear();

    result
}

fn edit_existing(
    id: TaskId,
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<()> {
    let task = store.get_task(id)?;
    let mut baseline = task.clone();
    let mut save = |proposed: Task| -> Result<Task> {
        if proposed == baseline {
            return Ok(proposed);
        }
        // Ord changes go through `reorder_task` so other tasks shift correctly.
        // Other fields flow through update_task_with_revert.
        let mut for_update = proposed.clone();
        let target_ord_change = if proposed.ord != baseline.ord {
            Some(proposed.ord)
        } else {
            None
        };
        if target_ord_change.is_some() {
            for_update.ord = baseline.ord;
        }
        if for_update != baseline {
            store.update_task_with_revert(baseline.clone(), for_update.clone(), clock)?;
        }
        if let Some(target_ord) = target_ord_change {
            store.reorder_task(id, target_ord, clock)?;
        }
        baseline = store.get_task(id)?;
        Ok(baseline.clone())
    };
    editor.edit(&task, &mut save)
}

fn add_new(store: &mut Store, clock: &dyn Clock, editor: &dyn TaskEditor) -> Result<()> {
    let now = clock.now();
    let next_ord = store.next_active_ord()?;
    let template = Task {
        id: 0,
        text: String::new(),
        category: Category::B,
        ord: next_ord,
        est_secs: 1800,
        status: Status::Active,
        created_at: now,
        updated_at: now,
        completed_at: None,
        deleted_at: None,
    };
    let mut baseline: Option<Task> = None;
    let mut save = |proposed: Task| -> Result<Task> {
        match &baseline {
            None => {
                let mut t = proposed;
                t.id = store.next_id()?;
                let created = store.add_task_with_revert(t, clock)?;
                baseline = Some(created.clone());
                Ok(created)
            }
            Some(prev) => {
                if &proposed == prev {
                    return Ok(proposed);
                }
                store.update_task_with_revert(prev.clone(), proposed.clone(), clock)?;
                baseline = Some(proposed.clone());
                Ok(proposed)
            }
        }
    };
    editor.edit(&template, &mut save)
}

fn refresh_tasks(app: &mut App, store: &Store) -> Result<()> {
    let cursor_id = app.cursor_task().map(|t| t.id);
    app.tasks = load_active_tasks(store)?;
    let active_ids: std::collections::HashSet<TaskId> = app.tasks.iter().map(|t| t.id).collect();
    app.pending.retain(|id, _| active_ids.contains(id));
    if let Some(id) = cursor_id {
        if let Some(pos) = app.filtered_tasks().iter().position(|t| t.id == id) {
            app.cursor = pos;
            return Ok(());
        }
    }
    app.clamp_cursor();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    fn at(year: i32, month: u32, day: u32, hour: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap()
    }

    fn open_store_with_task(text: &str) -> (tempfile::TempDir, Store, FakeClock, TaskId) {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = FakeClock::new(at(2026, 5, 17, 12));
        let task = crate::commands::add::run(&[text.to_string()], &mut store, &clock).unwrap();
        (dir, store, clock, task.id)
    }

    fn make_app(store: &Store) -> App {
        App::new(load_active_tasks(store).unwrap())
    }

    #[test]
    fn refresh_tasks_picks_up_externally_added_task() {
        let (_dir, mut store, clock, _id) = open_store_with_task("first");
        let mut app = make_app(&store);
        assert_eq!(app.tasks.len(), 1);
        crate::commands::add::run(&["second".into()], &mut store, &clock).unwrap();
        refresh_tasks(&mut app, &store).unwrap();
        assert_eq!(app.tasks.len(), 2);
    }

    #[test]
    fn refresh_tasks_drops_externally_completed_task() {
        let (_dir, mut store, clock, id) = open_store_with_task("chores");
        let mut app = make_app(&store);
        crate::commands::complete::run(id, &mut store, &clock).unwrap();
        refresh_tasks(&mut app, &store).unwrap();
        assert!(app.tasks.is_empty());
    }

    #[test]
    fn refresh_tasks_preserves_cursor_on_same_task_id() {
        let (_dir, mut store, clock, t1) = open_store_with_task("one");
        crate::commands::add::run(&["two".into()], &mut store, &clock).unwrap();
        let mut app = make_app(&store);
        app.cursor = 1;
        crate::commands::delete::run(t1, &mut store, &clock).unwrap();
        refresh_tasks(&mut app, &store).unwrap();
        assert_eq!(app.tasks.len(), 1);
        assert_eq!(app.cursor, 0);
        assert_eq!(app.cursor_task().unwrap().text, "two");
    }

    #[test]
    fn refresh_tasks_drops_pending_changes_for_vanished_tasks() {
        let (_dir, mut store, clock, id) = open_store_with_task("chores");
        let mut app = make_app(&store);
        app.pending
            .entry(id)
            .or_default()
            .push(crate::tui::events::PendingChange::ToggleComplete(id));
        crate::commands::delete::run(id, &mut store, &clock).unwrap();
        refresh_tasks(&mut app, &store).unwrap();
        assert!(app.pending.is_empty());
    }
}
