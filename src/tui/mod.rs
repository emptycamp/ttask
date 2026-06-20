pub mod events;
pub mod render;

use crate::clock::Clock;
use crate::editor::TaskEditor;
use crate::error::{Error, Result};
use crate::format::sort_key;
use crate::model::{Category, Status, Task, TaskId};
use crate::store::{Store, StoreSnapshot};
use crate::tui::events::Action;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::time::{Duration as StdDuration, Instant};

const EVENT_POLL_MS: u64 = 100;
/// How often to re-read the store so external `ttask add/edit/delete/complete`
/// invocations show up live in the open TUI. 500 ms balances "feels live" with
/// "don't spam the store with read txns".
const EXTERNAL_REFRESH_MS: u64 = 500;

pub struct App {
    pub tasks: Vec<Task>,
    pub cursor: usize,
    /// `Some(buf)` while the user is editing the search prompt; `None` when not in
    /// search-input mode. While editing, the in-progress buffer is also the live
    /// filter applied to the displayed list.
    pub search_input: Option<String>,
    /// The committed filter — survives across input/exit transitions. Empty means
    /// no filter is applied. Matches case-insensitively against task text.
    pub search_filter: String,
    /// Last user-facing status note (e.g. "undone", "nothing to redo"), shown in
    /// the footer until the next action.
    pub status: Option<String>,
}

impl App {
    pub fn new(tasks: Vec<Task>) -> Self {
        Self {
            tasks,
            cursor: 0,
            search_input: None,
            search_filter: String::new(),
            status: None,
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

/// In-session undo/redo of immediate store mutations. Each entry is a full store
/// snapshot taken just *before* a mutation; undo restores the previous snapshot
/// (capturing the current state for redo). Both stacks are dropped when the TUI
/// exits, so once the user reopens `ttask` the only way to roll back further is
/// `ttask history`.
struct UndoStacks {
    undo: Vec<StoreSnapshot>,
    redo: Vec<StoreSnapshot>,
}

impl UndoStacks {
    fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

pub fn run(store: &mut Store, clock: &dyn Clock, editor: &dyn TaskEditor) -> Result<()> {
    let mut app = App::new(load_active_tasks(store)?);
    // `enter`/`leave` bracket the whole session and balance even if `run_on_screen`
    // bails out building the terminal, so the screen is always restored on exit.
    crate::screen::enter()?;
    let result = run_on_screen(&mut app, store, clock, editor);
    crate::screen::leave();
    result
}

fn run_on_screen(
    app: &mut App,
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<()> {
    let mut terminal = build_terminal()?;
    run_loop(&mut terminal, app, store, clock, editor)
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

/// Repaint the list after a nested full-screen UI (editor or link picker) drew over
/// our screen. Those UIs use their own `Terminal`, so ours believes the list is still
/// on screen and would skip the redraw; `clear` forces a full repaint. Drawing again
/// right away keeps the blank from being visible.
fn force_redraw(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &App) -> Result<()> {
    terminal.clear().map_err(Error::Io)?;
    terminal.draw(|f| render::draw(f, app)).map_err(Error::Io)?;
    Ok(())
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<()> {
    let mut stacks = UndoStacks::new();
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
                app.status = None;
                run_mutation(store, &mut stacks, |store| {
                    edit_existing(id, store, clock, editor)
                })?;
                refresh_tasks(app, store)?;
                // The editor drew over our screen (sharing the alternate screen), so
                // repaint the list cleanly over it.
                force_redraw(terminal, app)?;
            }
            Action::AddTask => {
                app.status = None;
                let new_id =
                    run_mutation(store, &mut stacks, |store| add_new(store, clock, editor))?;
                refresh_tasks(app, store)?;
                // Land the cursor on the freshly created task so it can be acted
                // on immediately without navigating.
                if let Some(id) = new_id {
                    if let Some(pos) = app.filtered_tasks().iter().position(|t| t.id == id) {
                        app.cursor = pos;
                    }
                }
                force_redraw(terminal, app)?;
            }
            Action::ReorderCursor(target_ord) => {
                if let Some(id) = app.cursor_task().map(|t| t.id) {
                    app.status = None;
                    run_mutation(store, &mut stacks, |store| {
                        store.reorder_task(id, target_ord, clock)
                    })?;
                    refresh_tasks(app, store)?;
                    if let Some(p) = app.filtered_tasks().iter().position(|t| t.id == id) {
                        app.cursor = p;
                    }
                }
            }
            Action::SetCategory(id, category) => {
                app.status = None;
                run_mutation(store, &mut stacks, |store| {
                    set_category(id, category, store, clock)
                })?;
                refresh_tasks(app, store)?;
            }
            Action::Complete(id) => {
                app.status = None;
                run_mutation(store, &mut stacks, |store| {
                    crate::commands::complete::run(id, store, clock)
                })?;
                refresh_tasks(app, store)?;
            }
            Action::Delete(id) => {
                app.status = None;
                run_mutation(store, &mut stacks, |store| {
                    crate::commands::delete::run(id, store, clock)
                })?;
                refresh_tasks(app, store)?;
            }
            Action::OpenLink(id) => {
                app.status = None;
                // Opening a link doesn't touch the store, so it's not an undoable
                // mutation. A single link opens straight away without drawing anything;
                // only the multi-link picker takes over the screen, so only then do we
                // need to repaint afterwards.
                let needs_repaint = open_shows_picker(store, id);
                let store_ref: &Store = store;
                let outcome =
                    crate::commands::open::run(id, None, store_ref, &crate::commands::SystemTty);
                app.status = Some(match outcome {
                    Ok(Some(url)) => format!("opened {url}"),
                    Ok(None) => "open cancelled".to_string(),
                    Err(e) => format!("{e}"),
                });
                if needs_repaint {
                    force_redraw(terminal, app)?;
                }
            }
            Action::Undo => {
                undo(store, &mut stacks, app)?;
            }
            Action::Redo => {
                redo(store, &mut stacks, app)?;
            }
        }
    }
}

/// Run a store mutation, recording an undo checkpoint if it actually changed
/// anything. Every TUI mutation pushes a history event, so a bump in the event
/// sequence is a reliable "something happened" signal — cheaper than diffing the
/// whole snapshot and it skips no-op edits (e.g. the user cancelled the editor).
fn run_mutation<F, T>(store: &mut Store, stacks: &mut UndoStacks, f: F) -> Result<T>
where
    F: FnOnce(&mut Store) -> Result<T>,
{
    let before = store.snapshot()?;
    let seq_before = store.current_seq()?;
    let out = f(store)?;
    if store.current_seq()? != seq_before {
        stacks.undo.push(before);
        stacks.redo.clear();
    }
    Ok(out)
}

fn undo(store: &mut Store, stacks: &mut UndoStacks, app: &mut App) -> Result<()> {
    match stacks.undo.pop() {
        Some(prev) => {
            let current = store.snapshot()?;
            store.restore(&prev)?;
            stacks.redo.push(current);
            app.status = Some("undone".into());
            refresh_tasks(app, store)?;
        }
        None => app.status = Some("nothing to undo".into()),
    }
    Ok(())
}

fn redo(store: &mut Store, stacks: &mut UndoStacks, app: &mut App) -> Result<()> {
    match stacks.redo.pop() {
        Some(next) => {
            let current = store.snapshot()?;
            store.restore(&next)?;
            stacks.undo.push(current);
            app.status = Some("redone".into());
            refresh_tasks(app, store)?;
        }
        None => app.status = Some("nothing to redo".into()),
    }
    Ok(())
}

fn set_category(
    id: TaskId,
    category: Category,
    store: &mut Store,
    clock: &dyn Clock,
) -> Result<()> {
    let before = store.get_task(id)?;
    if before.category == category {
        return Ok(());
    }
    let mut after = before.clone();
    after.category = category;
    // A category change is a user touch, so reset the GC stale-clock like an edit.
    after.updated_at = clock.now();
    store.update_task_with_revert(before, after, clock)
}

/// Whether opening task `id` would pop the interactive link picker — i.e. it has
/// more than one link. A single link (or none) never draws, so the caller can skip
/// the post-open repaint and keep the screen perfectly still.
fn open_shows_picker(store: &Store, id: TaskId) -> bool {
    store
        .get_task(id)
        .map(|t| crate::commands::open::extract_links(&t.text).len() > 1)
        .unwrap_or(false)
}

fn edit_existing(
    id: TaskId,
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<()> {
    let task = store.get_task(id)?;
    let baseline = task.clone();
    // The edit TUI only ever changes text and estimate, so a single mutate covers
    // it. (Category and ord are changed from the main view, not the editor.)
    let mut save = |proposed: Task| -> Result<Task> {
        if proposed == baseline {
            return Ok(proposed);
        }
        store.update_task_with_revert(baseline.clone(), proposed.clone(), clock)?;
        store.get_task(id)
    };
    editor.edit(&task, &mut save)
}

/// Returns the id of the created task, or `None` if the editor was cancelled
/// before anything was saved (so the caller can move the cursor onto it). Shares the
/// form path with `ttask add` (no args) via `commands::add::run_form`.
fn add_new(
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<Option<TaskId>> {
    Ok(crate::commands::add::run_form(store, clock, editor)?.map(|t| t.id))
}

fn refresh_tasks(app: &mut App, store: &Store) -> Result<()> {
    let cursor_id = app.cursor_task().map(|t| t.id);
    app.tasks = load_active_tasks(store)?;
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

    /// Editor stub that saves a single task with the given text (used to drive
    /// `add_new` without a TTY).
    struct SavingEditor {
        text: String,
    }
    impl TaskEditor for SavingEditor {
        fn edit(&self, task: &Task, save: &mut crate::editor::Saver<'_>) -> Result<()> {
            let mut t = task.clone();
            t.text = self.text.clone();
            save(t)?;
            Ok(())
        }
    }

    /// Editor stub that cancels without saving.
    struct CancellingEditor;
    impl TaskEditor for CancellingEditor {
        fn edit(&self, _task: &Task, _save: &mut crate::editor::Saver<'_>) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn add_new_returns_created_task_id() {
        let (_dir, mut store, clock, _id) = open_store_with_task("first");
        let editor = SavingEditor {
            text: "second".into(),
        };
        let new_id = add_new(&mut store, &clock, &editor).unwrap();
        let id = new_id.expect("a task should have been created");
        assert_eq!(store.get_task(id).unwrap().text, "second");
    }

    #[test]
    fn add_new_returns_none_when_editor_cancels() {
        let (_dir, mut store, clock, _id) = open_store_with_task("first");
        assert!(add_new(&mut store, &clock, &CancellingEditor)
            .unwrap()
            .is_none());
    }

    #[test]
    fn cursor_lands_on_newly_added_task() {
        // Existing task sorts before the new one (same category B, lower ord), so
        // the cursor only ends on the new task if we explicitly move it there.
        let (_dir, mut store, clock, _id) = open_store_with_task("aaa first");
        let mut app = make_app(&store);
        let editor = SavingEditor {
            text: "zzz second".into(),
        };
        let new_id = add_new(&mut store, &clock, &editor).unwrap();
        refresh_tasks(&mut app, &store).unwrap();
        if let Some(id) = new_id {
            if let Some(pos) = app.filtered_tasks().iter().position(|t| t.id == id) {
                app.cursor = pos;
            }
        }
        assert_eq!(app.cursor_task().map(|t| t.id), new_id);
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
    fn run_mutation_records_undo_only_when_state_changes() {
        let (_dir, mut store, clock, id) = open_store_with_task("chores");
        let mut stacks = UndoStacks::new();

        // A real mutation (delete) should record an undo checkpoint.
        run_mutation(&mut store, &mut stacks, |s| {
            crate::commands::delete::run(id, s, &clock)
        })
        .unwrap();
        assert_eq!(stacks.undo.len(), 1);

        // A no-op closure should not.
        run_mutation(&mut store, &mut stacks, |_s| Ok(())).unwrap();
        assert_eq!(stacks.undo.len(), 1);
    }

    #[test]
    fn undo_then_redo_round_trips_a_delete() {
        let (_dir, mut store, clock, id) = open_store_with_task("chores");
        let mut app = make_app(&store);
        let mut stacks = UndoStacks::new();

        run_mutation(&mut store, &mut stacks, |s| {
            crate::commands::delete::run(id, s, &clock)
        })
        .unwrap();
        refresh_tasks(&mut app, &store).unwrap();
        assert!(app.tasks.is_empty(), "task should be gone after delete");

        undo(&mut store, &mut stacks, &mut app).unwrap();
        assert_eq!(app.tasks.len(), 1, "undo should bring the task back");
        assert_eq!(store.get_task(id).unwrap().status, Status::Active);

        redo(&mut store, &mut stacks, &mut app).unwrap();
        assert!(app.tasks.is_empty(), "redo should re-apply the delete");
    }

    #[test]
    fn undo_with_empty_stack_sets_status_note() {
        let (_dir, mut store, _clock, _id) = open_store_with_task("chores");
        let mut app = make_app(&store);
        let mut stacks = UndoStacks::new();
        undo(&mut store, &mut stacks, &mut app).unwrap();
        assert_eq!(app.status.as_deref(), Some("nothing to undo"));
    }
}
