use crate::clock::Clock;
use crate::editor::TaskEditor;
use crate::error::{Error, Result};
use crate::model::{Status, TaskId};
use crate::store::{MutateKind, Store};
use crate::time::parse_fields::{parse_task_fields, ParsedFields};

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

    let ParsedFields {
        text,
        category,
        ord,
        est_secs,
    } = parse_task_fields(args)?;

    // The ord change is applied via `reorder_task` (with its shift math) so we
    // strip it from the in-place edit closure. Everything else flows through
    // `mutate_task` so we still get an "edited" history event for the other
    // field changes.
    let non_ord_change = text.is_some() || category.is_some() || est_secs.is_some();
    if non_ord_change {
        store.mutate_task(
            id,
            MutateKind::Edit,
            |before| {
                ensure_editable(before)?;
                let mut updated = before.clone();
                if let Some(t) = text {
                    updated.text = t;
                }
                if let Some(p) = category {
                    updated.category = p;
                }
                if let Some(e) = est_secs {
                    updated.est_secs = e;
                }
                Ok(updated)
            },
            clock,
        )?;
    } else {
        // Even when only the ord is changing, we still want to confirm the task
        // is editable before reordering.
        let task = store.get_task(id)?;
        ensure_editable(&task)?;
    }
    if let Some(target_ord) = ord {
        store.reorder_task(id, target_ord, clock)?;
    }
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
        let target_ord_change = {
            let current = store.get_task(id)?;
            if current.ord != proposed.ord {
                Some(proposed.ord)
            } else {
                None
            }
        };
        let proposed_for_mutate = {
            let mut p = proposed.clone();
            // Apply ord through reorder_task instead — strip it here so the
            // mutate path doesn't fight with the shift.
            if target_ord_change.is_some() {
                let current = store.get_task(id)?;
                p.ord = current.ord;
            }
            p
        };
        let persisted = store.mutate_task(
            id,
            MutateKind::Edit,
            |_current| Ok(proposed_for_mutate.clone()),
            clock,
        )?;
        if let Some(target_ord) = target_ord_change {
            store.reorder_task(id, target_ord, clock)?;
        }
        store.get_task(persisted.id)
    };
    editor.edit(&task, &mut save)
}

fn ensure_editable(task: &crate::model::Task) -> Result<()> {
    match task.status {
        Status::Active => Ok(()),
        Status::Completed => Err(Error::Parse(format!(
            "task #{} is completed; revert the completion via `ttask history` before editing",
            task.id
        ))),
        Status::SoftDeleted => Err(Error::Parse(format!(
            "task #{} is deleted; revert the deletion via `ttask history` before editing",
            task.id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::editor::Saver;
    use crate::model::{Category, Status, Task};
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    fn make_clock() -> FakeClock {
        FakeClock::new(Utc.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap())
    }

    fn make_task(id: u32) -> Task {
        let now = Utc::now();
        Task {
            id,
            text: "original".to_string(),
            category: Category::B,
            ord: id,
            est_secs: 1800,
            status: Status::Active,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        }
    }

    struct SaveOnceEditor {
        replacement: Task,
    }
    impl TaskEditor for SaveOnceEditor {
        fn edit(&self, _task: &Task, save: &mut Saver<'_>) -> Result<()> {
            save(self.replacement.clone())?;
            Ok(())
        }
    }

    struct CancelEditor;
    impl TaskEditor for CancelEditor {
        fn edit(&self, _task: &Task, _save: &mut Saver<'_>) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn edit_category_via_args() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let clock = make_clock();
        let mut t = make_task(1);
        t.category = Category::A;
        run(
            1,
            &["p:a".to_string()],
            &mut store,
            &clock,
            &SaveOnceEditor { replacement: t },
        )
        .unwrap();
        let updated = store.get_task(1).unwrap();
        assert_eq!(updated.category, Category::A);
    }

    #[test]
    fn edit_text_via_args() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let clock = make_clock();
        run(
            1,
            &["new text".to_string()],
            &mut store,
            &clock,
            &CancelEditor,
        )
        .unwrap();
        let updated = store.get_task(1).unwrap();
        assert_eq!(updated.text, "new text");
    }

    #[test]
    fn edit_ord_via_args_reorders_tasks() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        let mut t1 = make_task(1);
        t1.ord = 1;
        let mut t2 = make_task(2);
        t2.ord = 2;
        let mut t3 = make_task(3);
        t3.ord = 3;
        store.add_task(t1).unwrap();
        store.add_task(t2).unwrap();
        store.add_task(t3).unwrap();

        // Move task #3 to ord 1.
        run(3, &["ord:1".to_string()], &mut store, &clock, &CancelEditor).unwrap();
        assert_eq!(store.get_task(3).unwrap().ord, 1);
        assert_eq!(store.get_task(1).unwrap().ord, 2);
        assert_eq!(store.get_task(2).unwrap().ord, 3);
    }

    #[test]
    fn edit_nonexistent_task_returns_error() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        assert!(run(99, &["p:a".to_string()], &mut store, &clock, &CancelEditor).is_err());
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
}
