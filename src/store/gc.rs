use crate::clock::Clock;
use crate::error::Result;
use crate::model::{Category, Status};
use crate::store::workdays::work_days_between;
use crate::store::Store;

/// Auto-deletion thresholds. All counted in **working days** (Mon–Fri local).
/// Active tasks measure age from `updated_at` so any user edit (text, category,
/// ord, est) resets the clock; touching a task keeps it alive. Completed and
/// soft-deleted tasks measure age from their transition timestamp.
pub const ACTIVE_C_MAX_WORK_DAYS: i64 = 2;
pub const ACTIVE_B_MAX_WORK_DAYS: i64 = 5;
pub const COMPLETED_MAX_WORK_DAYS: i64 = 5;
pub const DELETED_MAX_WORK_DAYS: i64 = 5;

/// Hard-delete tasks past their work-day threshold:
/// - Active C-category: 2 work days since last update
/// - Active B-category: 1 work week (5 work days) since last update
/// - Active A-category: never
/// - Completed: 1 work week (5 work days) since completion
/// - Soft-deleted: 1 work week (5 work days) since deletion
pub fn sweep(store: &mut Store, clock: &dyn Clock) -> Result<u32> {
    let now = clock.now();
    let tasks = store.all_tasks()?;
    let mut count = 0u32;

    for task in tasks {
        let should_delete = match (task.status, task.category) {
            (Status::Active, Category::C) => {
                work_days_between(task.updated_at, now) > ACTIVE_C_MAX_WORK_DAYS
            }
            (Status::Active, Category::B) => {
                work_days_between(task.updated_at, now) > ACTIVE_B_MAX_WORK_DAYS
            }
            (Status::Active, Category::A) => false,
            (Status::Completed, _) => task
                .completed_at
                .map(|t| work_days_between(t, now) > COMPLETED_MAX_WORK_DAYS)
                .unwrap_or(false),
            (Status::SoftDeleted, _) => task
                .deleted_at
                .map(|t| work_days_between(t, now) > DELETED_MAX_WORK_DAYS)
                .unwrap_or(false),
        };

        if should_delete {
            store.hard_delete(task.id)?;
            count += 1;
        }
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::model::{Category, Status, Task};
    use crate::store::Store;
    use chrono::{DateTime, Duration, Local, TimeZone, Utc};
    use tempfile::tempdir;

    /// Build a UTC instant pinned to a specific *local* date at noon — calendar
    /// math in `work_days_between` runs in local time, so anchoring on UTC
    /// midnight can leak the assertion onto the wrong weekday.
    fn local_noon(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Local
            .with_ymd_and_hms(year, month, day, 12, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc)
    }

    fn make_task(id: u32, category: Category, status: Status, updated_at: DateTime<Utc>) -> Task {
        Task {
            id,
            text: format!("task {id}"),
            category,
            ord: id,
            est_secs: 1800,
            status,
            created_at: updated_at,
            updated_at,
            completed_at: if status == Status::Completed {
                Some(updated_at)
            } else {
                None
            },
            deleted_at: if status == Status::SoftDeleted {
                Some(updated_at)
            } else {
                None
            },
        }
    }

    fn open_store(dir: &std::path::Path) -> Store {
        Store::open(dir).unwrap()
    }

    #[test]
    fn active_c_deleted_after_two_work_days_plus_one() {
        // Updated Mon 18 May; sweep on Thu 21 May → 3 work days elapsed > 2.
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let updated = local_noon(2026, 5, 18); // Mon
        store
            .add_task(make_task(1, Category::C, Status::Active, updated))
            .unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 21)); // Thu (+3 work days)
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 1);
        assert!(store.get_task(1).is_err());
    }

    #[test]
    fn active_c_survives_exactly_two_work_days() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let updated = local_noon(2026, 5, 18); // Mon
        store
            .add_task(make_task(1, Category::C, Status::Active, updated))
            .unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 20)); // Wed (+2 work days)
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 0);
        assert!(store.get_task(1).is_ok());
    }

    #[test]
    fn active_c_weekend_does_not_age_the_task() {
        // Updated Fri; sweep Mon — only 1 work day elapsed, well under the cap.
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let updated = local_noon(2026, 5, 22); // Fri
        store
            .add_task(make_task(1, Category::C, Status::Active, updated))
            .unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 25)); // Mon
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn active_b_survives_one_work_week() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let updated = local_noon(2026, 5, 18); // Mon
        store
            .add_task(make_task(1, Category::B, Status::Active, updated))
            .unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 25)); // Mon (+5 work days)
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn active_b_deleted_after_one_work_week_plus_one() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let updated = local_noon(2026, 5, 18); // Mon
        store
            .add_task(make_task(1, Category::B, Status::Active, updated))
            .unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 26)); // Tue (+6 work days)
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn active_a_never_deleted() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let updated = local_noon(2026, 1, 1);
        store
            .add_task(make_task(1, Category::A, Status::Active, updated))
            .unwrap();
        let clock = FakeClock::new(updated + Duration::days(365));
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn completed_deleted_after_one_work_week() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let base = local_noon(2026, 5, 18); // Mon
        let mut task = make_task(1, Category::B, Status::Completed, base);
        task.completed_at = Some(base);
        store.add_task(task).unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 26)); // Tue (+6 work days)
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn completed_survives_one_work_week_exactly() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let base = local_noon(2026, 5, 18); // Mon
        let mut task = make_task(1, Category::B, Status::Completed, base);
        task.completed_at = Some(base);
        store.add_task(task).unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 25)); // Mon (+5 work days)
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn soft_deleted_hard_deleted_after_one_work_week() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let base = local_noon(2026, 5, 18); // Mon
        let mut task = make_task(1, Category::B, Status::SoftDeleted, base);
        task.deleted_at = Some(base);
        store.add_task(task).unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 26)); // Tue (+6 work days)
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn soft_deleted_survives_one_work_week_exactly() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let base = local_noon(2026, 5, 18); // Mon
        let mut task = make_task(1, Category::B, Status::SoftDeleted, base);
        task.deleted_at = Some(base);
        store.add_task(task).unwrap();
        let clock = FakeClock::new(local_noon(2026, 5, 25)); // Mon (+5 work days)
        let count = sweep(&mut store, &clock).unwrap();
        assert_eq!(count, 0);
    }
}
