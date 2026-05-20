use crate::clock::Clock;
use crate::editor::TaskEditor;
use crate::error::{Error, Result};
use crate::model::{Status, TaskId};
use crate::store::{MutateKind, Store};
use crate::time::parse_fields::{parse_task_fields, ParsedFields};
use chrono::Local;

pub fn run(
    id: TaskId,
    args: &[String],
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<()> {
    if args.is_empty() {
        return run_form(id, store, clock, editor);
    }

    let now_utc = clock.now();
    let now_local: chrono::DateTime<Local> = now_utc.into();

    let ParsedFields {
        text,
        priority,
        due,
        est_secs,
    } = parse_task_fields(args, now_local)?;

    store.mutate_task(
        id,
        MutateKind::Edit,
        |before| {
            ensure_editable(before)?;
            let mut updated = before.clone();
            if let Some(t) = text {
                updated.text = t;
            }
            if let Some(p) = priority {
                updated.priority = p;
            }
            if let Some(d) = due {
                updated.due = d.with_timezone(&chrono::Utc);
            }
            if let Some(e) = est_secs {
                updated.est_secs = e;
            }
            Ok(updated)
        },
        clock,
    )?;
    Ok(())
}

fn run_form(
    id: TaskId,
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
) -> Result<()> {
    let task = store.get_task(id)?;
    ensure_editable(&task)?;
    let mut save = |proposed: crate::model::Task| -> Result<crate::model::Task> {
        // Read the actual current state inside the write txn — the form editor's idea
        // of "baseline" may be stale if another process touched the task in the
        // meantime, and we want the recorded `before` to match what we're actually
        // overwriting.
        store.mutate_task(
            id,
            MutateKind::Edit,
            |_current| Ok(proposed.clone()),
            clock,
        )
    };
    editor.edit(&task, &mut save)
}

fn ensure_editable(task: &crate::model::Task) -> Result<()> {
    match task.status {
        Status::Active => Ok(()),
        Status::Completed => Err(Error::Parse(format!(
            "task #{} is completed; revert the completion via `task history` before editing",
            task.id
        ))),
        Status::SoftDeleted => Err(Error::Parse(format!(
            "task #{} is deleted; revert the deletion via `task history` before editing",
            task.id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::editor::Saver;
    use crate::model::{Priority, Status, Task};
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    fn make_clock() -> FakeClock {
        FakeClock::new(Utc.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap())
    }

    fn make_task(id: u32) -> Task {
        Task {
            id,
            text: "original".to_string(),
            priority: Priority::B,
            due: Utc::now(),
            est_secs: 1800,
            status: Status::Active,
            created_at: Utc::now(),
            completed_at: None,
            deleted_at: None,
        }
    }

    /// Test editor that calls save once with a pre-baked replacement.
    struct SaveOnceEditor {
        replacement: Task,
    }
    impl TaskEditor for SaveOnceEditor {
        fn edit(&self, _task: &Task, save: &mut Saver<'_>) -> Result<()> {
            save(self.replacement.clone())?;
            Ok(())
        }
    }

    /// Test editor that never calls save (cancel).
    struct CancelEditor;
    impl TaskEditor for CancelEditor {
        fn edit(&self, _task: &Task, _save: &mut Saver<'_>) -> Result<()> {
            Ok(())
        }
    }

    /// Test editor that saves twice — simulates :w followed by another :w.
    struct SaveTwiceEditor {
        first: Task,
        second: Task,
    }
    impl TaskEditor for SaveTwiceEditor {
        fn edit(&self, _task: &Task, save: &mut Saver<'_>) -> Result<()> {
            save(self.first.clone())?;
            save(self.second.clone())?;
            Ok(())
        }
    }

    #[test]
    fn edit_priority_via_args() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let clock = make_clock();
        let mut t = make_task(1);
        t.priority = Priority::A;
        run(1, &["p:a".to_string()], &mut store, &clock, &SaveOnceEditor { replacement: t }).unwrap();
        let updated = store.get_task(1).unwrap();
        assert_eq!(updated.priority, Priority::A);
    }

    #[test]
    fn edit_text_via_args() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let clock = make_clock();
        run(1, &["new text".to_string()], &mut store, &clock, &CancelEditor).unwrap();
        let updated = store.get_task(1).unwrap();
        assert_eq!(updated.text, "new text");
    }

    #[test]
    fn edit_nonexistent_task_returns_error() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        assert!(run(99, &["p:a".to_string()], &mut store, &clock, &CancelEditor).is_err());
    }

    #[test]
    fn edit_no_args_runs_form_editor_and_persists_save() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let clock = make_clock();
        let mut replacement = make_task(1);
        replacement.text = "from form editor".into();
        run(1, &[], &mut store, &clock, &SaveOnceEditor { replacement }).unwrap();
        let updated = store.get_task(1).unwrap();
        assert_eq!(updated.text, "from form editor");
    }

    #[test]
    fn edit_form_cancel_leaves_task_unchanged() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let clock = make_clock();
        run(1, &[], &mut store, &clock, &CancelEditor).unwrap();
        let task = store.get_task(1).unwrap();
        assert_eq!(task.text, "original");
    }

    #[test]
    fn edit_form_two_saves_persist_both() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let clock = make_clock();
        let mut first = make_task(1);
        first.text = "first save".into();
        let mut second = make_task(1);
        second.text = "second save".into();
        run(
            1,
            &[],
            &mut store,
            &clock,
            &SaveTwiceEditor { first, second },
        )
        .unwrap();
        let final_task = store.get_task(1).unwrap();
        assert_eq!(final_task.text, "second save");
        // Two history entries — one per save.
        let history = store.history().unwrap();
        assert_eq!(history.len(), 2);
    }
}
