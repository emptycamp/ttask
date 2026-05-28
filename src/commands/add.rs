use crate::clock::Clock;
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

    // Snapshot the active tasks *before* the add so we can splice the new one in at
    // the requested ord, shifting the bystanders. Without an explicit ord the task
    // is appended to the end.
    let active_before: Vec<Task> = store
        .all_tasks()?
        .into_iter()
        .filter(|t| t.status == Status::Active)
        .collect();
    let next_default_ord = store.next_active_ord()?;
    let requested_ord = ord.unwrap_or(next_default_ord);

    let created = store.add_task_atomic(
        |id, _| Task {
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
        // Shift the bystanders so the new task lands exactly at requested_ord. The
        // `compute_reorder` helper takes the full new task set sorted by current
        // ord — the new task is currently at `requested_ord`, possibly colliding
        // with an existing task at that ord.
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
