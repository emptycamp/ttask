use crate::confirm::Prompt;
use crate::error::{Error, Result};
use crate::store::{ClearStats, Store};

pub fn run(yes: bool, store: &mut Store, prompt: &dyn Prompt) -> Result<ClearStats> {
    if !yes && !prompt.confirm(&warning())? {
        return Err(Error::Cancelled);
    }
    store.clear_all()
}

fn warning() -> String {
    "WARNING: this deletes ALL tasks and ALL history. It cannot be undone.\n\
     Continue?"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::confirm::AutoConfirm;
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
            text: format!("task {id}"),
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

    /// Prompt that always returns false, simulating a user typing "n".
    struct AlwaysDeny;
    impl Prompt for AlwaysDeny {
        fn confirm(&self, _msg: &str) -> Result<bool> {
            Ok(false)
        }
    }

    #[test]
    fn clear_wipes_tasks_and_history() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        store.add_task_with_revert(make_task(1), &clock).unwrap();
        store.add_task_with_revert(make_task(2), &clock).unwrap();

        let stats = run(true, &mut store, &AutoConfirm).unwrap();
        assert_eq!(stats.tasks_cleared, 2);
        assert_eq!(stats.events_cleared, 2);

        assert!(store.all_tasks().unwrap().is_empty());
        assert!(store.history().unwrap().is_empty());
    }

    #[test]
    fn clear_returns_cancelled_on_no() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        store.add_task_with_revert(make_task(1), &clock).unwrap();

        let err = run(false, &mut store, &AlwaysDeny).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
        // The task is still there.
        assert_eq!(store.all_tasks().unwrap().len(), 1);
    }

    #[test]
    fn clear_skips_prompt_with_yes() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        store.add_task_with_revert(make_task(1), &clock).unwrap();
        // AlwaysDeny would normally refuse, but yes=true must bypass.
        let stats = run(true, &mut store, &AlwaysDeny).unwrap();
        assert_eq!(stats.tasks_cleared, 1);
    }

    #[test]
    fn clear_on_empty_store_succeeds() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let stats = run(true, &mut store, &AutoConfirm).unwrap();
        assert_eq!(stats.tasks_cleared, 0);
        assert_eq!(stats.events_cleared, 0);
    }
}
