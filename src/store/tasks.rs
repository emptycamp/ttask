use crate::error::{Error, Result};
use crate::model::{Status, Task, TaskId};
use crate::store::codec::Bincode;
use heed::types::U32;
use heed::{Database, RwTxn};

pub type TasksDb = Database<U32<heed::byteorder::BigEndian>, Bincode<Task>>;

pub fn next_id(txn: &heed::RoTxn<'_>, db: TasksDb) -> Result<TaskId> {
    let mut expected: u32 = 1;
    for result in db.iter(txn)? {
        let (key, _) = result?;
        if key != expected {
            return Ok(expected);
        }
        expected += 1;
    }
    Ok(expected)
}

/// Next available ord for an active task — one greater than the max active ord, or
/// 1 if there are no active tasks yet.
pub fn next_active_ord(txn: &heed::RoTxn<'_>, db: TasksDb) -> Result<u32> {
    let mut max_ord: u32 = 0;
    for result in db.iter(txn)? {
        let (_, t) = result?;
        if t.status == Status::Active && t.ord > max_ord {
            max_ord = t.ord;
        }
    }
    Ok(max_ord + 1)
}

pub fn put(txn: &mut RwTxn<'_>, db: TasksDb, task: &Task) -> Result<()> {
    db.put(txn, &task.id, task).map_err(Error::Db)
}

pub fn get(txn: &heed::RoTxn<'_>, db: TasksDb, id: TaskId) -> Result<Task> {
    db.get(txn, &id)?.ok_or(Error::NotFound(id))
}

pub fn delete(txn: &mut RwTxn<'_>, db: TasksDb, id: TaskId) -> Result<bool> {
    Ok(db.delete(txn, &id)?)
}

pub fn all(txn: &heed::RoTxn<'_>, db: TasksDb) -> Result<Vec<Task>> {
    db.iter(txn)?
        .map(|r| r.map(|(_, t)| t).map_err(Error::Db))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status, Task};
    use crate::store::Store;
    use chrono::Utc;
    use tempfile::tempdir;

    fn make_task(id: TaskId) -> Task {
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
    fn next_id_starts_at_one_when_empty() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let id = store.next_id().unwrap();
        assert_eq!(id, 1);
    }

    #[test]
    fn next_id_reuses_lowest_gap() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        store.add_task(make_task(2)).unwrap();
        store.add_task(make_task(4)).unwrap();
        let id = store.next_id().unwrap();
        assert_eq!(id, 3);
    }

    #[test]
    fn next_id_extends_when_no_gap() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        store.add_task(make_task(2)).unwrap();
        store.add_task(make_task(3)).unwrap();
        let id = store.next_id().unwrap();
        assert_eq!(id, 4);
    }

    #[test]
    fn next_active_ord_starts_at_one_when_empty() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.next_active_ord().unwrap(), 1);
    }

    #[test]
    fn next_active_ord_ignores_completed_and_deleted() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let mut t = make_task(1);
        t.ord = 5;
        t.status = Status::Completed;
        store.add_task(t).unwrap();
        let mut t = make_task(2);
        t.ord = 8;
        t.status = Status::SoftDeleted;
        store.add_task(t).unwrap();
        // No active tasks — next ord should still be 1.
        assert_eq!(store.next_active_ord().unwrap(), 1);
    }

    #[test]
    fn next_active_ord_extends_above_max_active() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let mut t = make_task(1);
        t.ord = 3;
        store.add_task(t).unwrap();
        assert_eq!(store.next_active_ord().unwrap(), 4);
    }
}
