use crate::clock::Clock;
use crate::error::{Error, Result};
use crate::model::{Status, TaskId};
use crate::store::{MutateKind, Store};

pub fn run(id: TaskId, store: &mut Store, clock: &dyn Clock) -> Result<()> {
    let now = clock.now();
    store.mutate_task(
        id,
        MutateKind::Complete,
        |before| match before.status {
            Status::Completed => Err(Error::Parse(format!(
                "task #{} is already completed",
                before.id
            ))),
            Status::SoftDeleted => Err(Error::Parse(format!(
                "task #{} is deleted; revert the deletion before completing",
                before.id
            ))),
            Status::Active => {
                let mut after = before.clone();
                after.status = Status::Completed;
                after.completed_at = Some(now);
                Ok(after)
            }
        },
        clock,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
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

    #[test]
    fn complete_sets_completed_status() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let clock = make_clock();
        run(1, &mut store, &clock).unwrap();
        let updated = store.get_task(1).unwrap();
        assert_eq!(updated.status, Status::Completed);
        assert!(updated.completed_at.is_some());
    }

    #[test]
    fn complete_nonexistent_returns_error() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        assert!(run(99, &mut store, &clock).is_err());
    }

    #[test]
    fn complete_already_completed_returns_error() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let mut task = make_task(1);
        task.status = Status::Completed;
        task.completed_at = Some(Utc::now());
        store.add_task(task).unwrap();
        let clock = make_clock();
        let err = run(1, &mut store, &clock).unwrap_err();
        assert!(format!("{err}").contains("already completed"));
    }

    #[test]
    fn complete_deleted_returns_error() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let mut task = make_task(1);
        task.status = Status::SoftDeleted;
        task.deleted_at = Some(Utc::now());
        store.add_task(task).unwrap();
        let clock = make_clock();
        let err = run(1, &mut store, &clock).unwrap_err();
        assert!(format!("{err}").contains("deleted"));
    }
}
