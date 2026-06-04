use crate::clock::Clock;
use crate::editor::TaskEditor;
use crate::error::{Error, Result};
use crate::model::{Category, Status, Task};
use crate::store::order;
use crate::store::Store;
use crate::time::parse_fields::{parse_task_fields, ParsedFields};

pub fn run(args: &[String], store: &mut Store, clock: &dyn Clock) -> Result<Task> {
    let now_utc = clock.now();

    let ParsedFields {
        text,
        category,
        ord,
        est_secs,
    } = parse_task_fields(args)?;

    let text = text.ok_or_else(|| Error::Parse("task text is required".into()))?;
    let category = category.unwrap_or(Category::B);
    let est_secs = est_secs.unwrap_or(1800);

    // Order is per-category. Snapshot the active tasks *in this category* before the
    // add so we can splice the new one in at the requested ord, shifting only that
    // category's bystanders. Without an explicit ord the task is appended to the end
    // of its category.
    let active_before: Vec<Task> = store
        .all_tasks()?
        .into_iter()
        .filter(|t| t.status == Status::Active && t.category == category)
        .collect();
    let next_default_ord = store.next_active_ord(category)?;
    let requested_ord = ord.unwrap_or(next_default_ord);

    let created = store.add_task_atomic(
        |id| Task {
            id,
            text,
            category,
            ord: requested_ord,
            est_secs,
            status: Status::Active,
            created_at: now_utc,
            updated_at: now_utc,
            completed_at: None,
            deleted_at: None,
        },
        clock,
    )?;

    if ord.is_some() {
        // Shift the bystanders so the new task lands exactly at requested_ord within
        // its category. `compute_reorder` takes the category's task set sorted by
        // current ord — the new task is currently at `requested_ord`, possibly
        // colliding with an existing task at that ord.
        let mut active: Vec<Task> = active_before;
        active.push(created.clone());
        active.sort_by_key(|t| (t.ord, t.id));
        let new_orders = order::compute_reorder(&active, created.id, requested_ord);
        for t in active {
            if let Some(&new_ord) = new_orders.get(&t.id) {
                if new_ord != t.ord {
                    let mut updated = t.clone();
                    updated.ord = new_ord;
                    store.update_task(updated)?;
                }
            }
        }
    }

    store.get_task(created.id)
}

/// Add a new task through the built-in form editor (no field args). Returns the
/// created task, or `None` if the editor was cancelled before anything was saved.
/// This is what `task add` (with no args) and the `a` key in the `task` view both
/// use, so the new task lands as a revertable history event either way.
pub fn run_form(
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<Option<Task>> {
    let now = clock.now();
    let next_ord = store.next_active_ord(Category::B)?;
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
    let mut created: Option<Task> = None;
    {
        let mut save = |proposed: Task| -> Result<Task> {
            match &created {
                // First save assigns the real id and inserts the task.
                None => {
                    let mut t = proposed;
                    t.id = store.next_id()?;
                    let saved = store.add_task_with_revert(t, clock)?;
                    created = Some(saved.clone());
                    Ok(saved)
                }
                // Subsequent saves (`:w` again) update the already-created task.
                Some(prev) => {
                    if &proposed == prev {
                        return Ok(proposed);
                    }
                    store.update_task_with_revert(prev.clone(), proposed.clone(), clock)?;
                    created = Some(proposed.clone());
                    Ok(proposed)
                }
            }
        };
        editor.edit(&template, &mut save)?;
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    fn make_clock() -> FakeClock {
        FakeClock::new(Utc.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap())
    }

    fn open_store(dir: &std::path::Path) -> Store {
        Store::open(dir).unwrap()
    }

    /// Editor stub that saves a single task with the given text.
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
    fn run_form_creates_task_from_editor() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let editor = SavingEditor {
            text: "From the editor".into(),
        };
        let created = run_form(&mut store, &clock, &editor).unwrap();
        let task = created.expect("a task should have been created");
        assert_eq!(task.text, "From the editor");
        assert_eq!(task.category, Category::B);
        assert_eq!(store.get_task(task.id).unwrap().text, "From the editor");
    }

    #[test]
    fn run_form_returns_none_when_cancelled() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        assert!(run_form(&mut store, &clock, &CancellingEditor)
            .unwrap()
            .is_none());
        assert!(store.all_tasks().unwrap().is_empty());
    }

    #[test]
    fn add_basic_task() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let args: Vec<String> = vec!["Buy milk".into()];
        let task = run(&args, &mut store, &clock).unwrap();
        assert_eq!(task.text, "Buy milk");
        assert_eq!(task.category, Category::B);
        assert_eq!(task.id, 1);
        assert_eq!(task.ord, 1);
    }

    #[test]
    fn add_task_with_category() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let args: Vec<String> = vec!["Read book".into(), "p:a".into()];
        let task = run(&args, &mut store, &clock).unwrap();
        assert_eq!(task.category, Category::A);
    }

    #[test]
    fn add_task_with_est() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let args: Vec<String> = vec!["Read book".into(), "est:1h".into()];
        let task = run(&args, &mut store, &clock).unwrap();
        assert_eq!(task.est_secs, 3600);
    }

    #[test]
    fn add_task_no_text_returns_error() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let args: Vec<String> = vec!["p:a".into()];
        assert!(run(&args, &mut store, &clock).is_err());
    }

    #[test]
    fn add_assigns_incremental_ids() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let t1 = run(&["Task one".to_string()], &mut store, &clock).unwrap();
        let t2 = run(&["Task two".to_string()], &mut store, &clock).unwrap();
        assert_eq!(t1.id, 1);
        assert_eq!(t2.id, 2);
        assert_eq!(t1.ord, 1);
        assert_eq!(t2.ord, 2);
    }

    #[test]
    fn add_orders_are_per_category() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        // Two B tasks, then an A task: A starts its own sequence at 1.
        let b1 = run(&["b one".into()], &mut store, &clock).unwrap();
        let b2 = run(&["b two".into()], &mut store, &clock).unwrap();
        let a1 = run(&["a one".into(), "c:a".into()], &mut store, &clock).unwrap();
        assert_eq!(b1.ord, 1);
        assert_eq!(b2.ord, 2);
        assert_eq!(a1.category, Category::A);
        assert_eq!(a1.ord, 1);
    }

    #[test]
    fn add_with_bare_trailing_duration_sets_estimate() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let t = run(
            &["Buy".into(), "milk".into(), "30m".into()],
            &mut store,
            &clock,
        )
        .unwrap();
        assert_eq!(t.text, "Buy milk");
        assert_eq!(t.est_secs, 1800);
    }

    #[test]
    fn add_with_ord_inserts_at_position_and_shifts_others() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let t1 = run(&["one".into()], &mut store, &clock).unwrap();
        let t2 = run(&["two".into()], &mut store, &clock).unwrap();
        // Insert the new task at ord=1; existing ords shift down.
        let new = run(&["new".into(), "ord:1".into()], &mut store, &clock).unwrap();
        assert_eq!(new.ord, 1);
        let t1_after = store.get_task(t1.id).unwrap();
        let t2_after = store.get_task(t2.id).unwrap();
        assert_eq!(t1_after.ord, 2);
        assert_eq!(t2_after.ord, 3);
    }
}
