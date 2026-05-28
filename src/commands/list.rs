use crate::error::Result;
use crate::format::{format_list, RenderOptions};
use crate::model::{Status, Task};
use crate::store::Store;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    Active,
    Completed,
    Deleted,
    All,
}

pub fn resolve_filter(_active: bool, completed: bool, deleted: bool, all: bool) -> Filter {
    // `_active` and the implicit default both resolve to Active, so there's no
    // dedicated arm for it — the final `else` covers both.
    if all {
        Filter::All
    } else if completed {
        Filter::Completed
    } else if deleted {
        Filter::Deleted
    } else {
        Filter::Active
    }
}

pub fn run(store: &Store, filter: Filter, opts: &RenderOptions) -> Result<String> {
    let tasks = store.all_tasks()?;

    let filtered: Vec<Task> = tasks
        .into_iter()
        .filter(|t| matches_filter(t.status, filter))
        .collect();

    Ok(format_list(&filtered, opts))
}

fn matches_filter(status: Status, filter: Filter) -> bool {
    match filter {
        Filter::Active => status == Status::Active,
        Filter::Completed => status == Status::Completed,
        Filter::Deleted => status == Status::SoftDeleted,
        Filter::All => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status, Task};
    use chrono::Utc;
    use tempfile::tempdir;

    fn make_task(id: u32, status: Status) -> Task {
        let now = Utc::now();
        Task {
            id,
            text: format!("task {id}"),
            category: Category::B,
            ord: id,
            est_secs: 1800,
            status,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn list_active_only_by_default() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1, Status::Active)).unwrap();
        store.add_task(make_task(2, Status::Completed)).unwrap();

        let opts = RenderOptions::no_color();
        let output = run(&store, Filter::Active, &opts).unwrap();
        assert!(output.contains("task 1"));
        assert!(!output.contains("task 2"));
    }

    #[test]
    fn list_completed_shows_only_completed() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1, Status::Active)).unwrap();
        store.add_task(make_task(2, Status::Completed)).unwrap();

        let opts = RenderOptions::no_color();
        let output = run(&store, Filter::Completed, &opts).unwrap();
        assert!(!output.contains("task 1"));
        assert!(output.contains("task 2"));
    }

    #[test]
    fn list_deleted_shows_only_deleted() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1, Status::Active)).unwrap();
        store.add_task(make_task(2, Status::SoftDeleted)).unwrap();

        let opts = RenderOptions::no_color();
        let output = run(&store, Filter::Deleted, &opts).unwrap();
        assert!(!output.contains("task 1"));
        assert!(output.contains("task 2"));
    }

    #[test]
    fn list_all_shows_every_status() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1, Status::Active)).unwrap();
        store.add_task(make_task(2, Status::Completed)).unwrap();
        store.add_task(make_task(3, Status::SoftDeleted)).unwrap();

        let opts = RenderOptions::no_color();
        let output = run(&store, Filter::All, &opts).unwrap();
        assert!(output.contains("task 1"));
        assert!(output.contains("task 2"));
        assert!(output.contains("task 3"));
    }

    #[test]
    fn resolve_filter_default_is_active() {
        assert_eq!(resolve_filter(false, false, false, false), Filter::Active);
    }

    #[test]
    fn resolve_filter_active_flag() {
        assert_eq!(resolve_filter(true, false, false, false), Filter::Active);
    }

    #[test]
    fn resolve_filter_completed_flag() {
        assert_eq!(resolve_filter(false, true, false, false), Filter::Completed);
    }

    #[test]
    fn resolve_filter_deleted_flag() {
        assert_eq!(resolve_filter(false, false, true, false), Filter::Deleted);
    }

    #[test]
    fn resolve_filter_all_flag() {
        assert_eq!(resolve_filter(false, false, false, true), Filter::All);
    }
}
