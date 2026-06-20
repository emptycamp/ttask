use chrono::{Duration, Local, TimeZone, Utc};
use tempfile::tempdir;
use ttask::clock::FakeClock;
use ttask::model::{Category, Status, Task};
use ttask::store::gc::sweep;
use ttask::store::Store;

fn make_task(
    id: u32,
    category: Category,
    status: Status,
    updated_at: chrono::DateTime<Utc>,
) -> Task {
    Task {
        id,
        text: format!("gc task {id}"),
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

/// Local-noon on a known weekday so calendar math is deterministic on any host.
fn local_noon(year: i32, month: u32, day: u32) -> chrono::DateTime<Utc> {
    Local
        .with_ymd_and_hms(year, month, day, 12, 0, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn gc_category_c_deleted_after_two_work_days_plus_one() {
    // Mon → Thu = 3 work days (>2 cap).
    let dir = tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .add_task(make_task(
            1,
            Category::C,
            Status::Active,
            local_noon(2026, 5, 18),
        ))
        .unwrap();
    let count = sweep(&mut store, &FakeClock::new(local_noon(2026, 5, 21))).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn gc_category_c_survives_weekend_only_age() {
    // Updated Fri; sweep Mon = 1 work day, well under 2.
    let dir = tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .add_task(make_task(
            1,
            Category::C,
            Status::Active,
            local_noon(2026, 5, 22),
        ))
        .unwrap();
    let count = sweep(&mut store, &FakeClock::new(local_noon(2026, 5, 25))).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn gc_category_b_survives_one_work_week_exactly() {
    let dir = tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .add_task(make_task(
            1,
            Category::B,
            Status::Active,
            local_noon(2026, 5, 18),
        ))
        .unwrap();
    // Mon → Mon = 5 work days, exactly the cap.
    let count = sweep(&mut store, &FakeClock::new(local_noon(2026, 5, 25))).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn gc_category_b_deleted_after_one_work_week_plus_one() {
    let dir = tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .add_task(make_task(
            1,
            Category::B,
            Status::Active,
            local_noon(2026, 5, 18),
        ))
        .unwrap();
    // Mon → next-Tue = 6 work days (>5 cap).
    let count = sweep(&mut store, &FakeClock::new(local_noon(2026, 5, 26))).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn gc_category_a_never_deleted() {
    let dir = tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let updated = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    store
        .add_task(make_task(1, Category::A, Status::Active, updated))
        .unwrap();
    let count = sweep(&mut store, &FakeClock::new(updated + Duration::days(365))).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn gc_completed_deleted_after_one_work_week_plus_one() {
    let dir = tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .add_task(make_task(
            1,
            Category::B,
            Status::Completed,
            local_noon(2026, 5, 18),
        ))
        .unwrap();
    let count = sweep(&mut store, &FakeClock::new(local_noon(2026, 5, 26))).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn gc_soft_deleted_hard_deleted_after_one_work_week_plus_one() {
    let dir = tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .add_task(make_task(
            1,
            Category::B,
            Status::SoftDeleted,
            local_noon(2026, 5, 18),
        ))
        .unwrap();
    let count = sweep(&mut store, &FakeClock::new(local_noon(2026, 5, 26))).unwrap();
    assert_eq!(count, 1);
}
