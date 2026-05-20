use crate::clock::Clock;
use crate::error::{Error, Result};
use crate::model::{Priority, Status, Task};
use crate::store::Store;
use crate::time::parse_fields::{parse_task_fields, ParsedFields};
use chrono::Local;

pub fn run(args: &[String], store: &mut Store, clock: &dyn Clock) -> Result<Task> {
    let now_utc = clock.now();
    let now_local: chrono::DateTime<Local> = now_utc.into();

    let ParsedFields {
        text,
        priority,
        due,
        est_secs,
    } = parse_task_fields(args, now_local)?;

    let text = text.ok_or_else(|| Error::Parse("task text is required".into()))?;
    let priority = priority.unwrap_or(Priority::B);
    let due = due
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| now_utc + chrono::Duration::minutes(5));
    let est_secs = est_secs.unwrap_or(1800);

    store.add_task_atomic(
        |id| Task {
            id,
            text,
            priority,
            due,
            est_secs,
            status: Status::Active,
            created_at: now_utc,
            completed_at: None,
            deleted_at: None,
        },
        clock,
    )
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
        assert_eq!(task.priority, Priority::B);
        assert_eq!(task.id, 1);
    }

    #[test]
    fn add_task_with_priority() {
        let dir = tempdir().unwrap();
        let mut store = open_store(dir.path());
        let clock = make_clock();
        let args: Vec<String> = vec!["Read book".into(), "p:a".into()];
        let task = run(&args, &mut store, &clock).unwrap();
        assert_eq!(task.priority, Priority::A);
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
    }
}
