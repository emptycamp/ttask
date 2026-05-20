use crate::error::{Error, Result};
use crate::model::{Task, TaskId};
use crate::store::codec::Bincode;
use chrono::{DateTime, Utc};
use heed::types::U64;
use heed::{Database, RoTxn, RwTxn};
use serde::{Deserialize, Serialize};

pub type RevertDb = Database<U64<heed::byteorder::BigEndian>, Bincode<HistoryEntry>>;
pub type MetaDb = Database<heed::types::Str, U64<heed::byteorder::BigEndian>>;

pub const MAX_HISTORY: usize = 30;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RevertOp {
    Added { id: TaskId },
    Edited { before: Task },
    Deleted { before: Task },
    Completed { before: Task },
}

impl RevertOp {
    pub fn summary(&self) -> String {
        match self {
            RevertOp::Added { id } => format!("added #{id}"),
            RevertOp::Edited { before } => format!("edited #{} ({})", before.id, truncate(&before.text, 30)),
            RevertOp::Deleted { before } => format!("deleted #{} ({})", before.id, truncate(&before.text, 30)),
            RevertOp::Completed { before } => format!("completed #{} ({})", before.id, truncate(&before.text, 30)),
        }
    }

    /// The task this operation affected. The cascade uses this so reverting an older
    /// event only pulls in newer events that touch the *same* task — separate tasks
    /// don't share history.
    pub fn task_id(&self) -> TaskId {
        match self {
            RevertOp::Added { id } => *id,
            RevertOp::Edited { before } => before.id,
            RevertOp::Deleted { before } => before.id,
            RevertOp::Completed { before } => before.id,
        }
    }
}

fn truncate(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        s.to_string()
    } else {
        format!("{}...", &chars[..width.saturating_sub(3)].iter().collect::<String>())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub op: RevertOp,
    pub timestamp: DateTime<Utc>,
}

const SEQ_KEY: &str = "revert_seq";

pub fn push(
    txn: &mut RwTxn<'_>,
    revert_db: RevertDb,
    meta_db: MetaDb,
    op: RevertOp,
    now: DateTime<Utc>,
) -> Result<()> {
    let seq = meta_db.get(txn, SEQ_KEY)?.unwrap_or(0) + 1;
    meta_db.put(txn, SEQ_KEY, &seq)?;
    let entry = HistoryEntry { op, timestamp: now };
    revert_db.put(txn, &seq, &entry).map_err(Error::Db)?;
    prune(txn, revert_db)?;
    Ok(())
}

fn prune(txn: &mut RwTxn<'_>, revert_db: RevertDb) -> Result<()> {
    let count = revert_db.len(txn)? as usize;
    if count <= MAX_HISTORY {
        return Ok(());
    }
    let to_remove = count - MAX_HISTORY;
    let keys: Vec<u64> = revert_db
        .iter(txn)?
        .take(to_remove)
        .map(|r| r.map(|(k, _)| k).map_err(Error::Db))
        .collect::<Result<Vec<_>>>()?;
    for key in keys {
        revert_db.delete(txn, &key)?;
    }
    Ok(())
}

pub fn list(txn: &RoTxn<'_>, revert_db: RevertDb) -> Result<Vec<(u64, HistoryEntry)>> {
    revert_db
        .iter(txn)?
        .map(|r| r.map_err(Error::Db))
        .collect()
}

pub fn get(txn: &RoTxn<'_>, revert_db: RevertDb, id: u64) -> Result<Option<HistoryEntry>> {
    Ok(revert_db.get(txn, &id)?)
}

pub fn delete(txn: &mut RwTxn<'_>, revert_db: RevertDb, id: u64) -> Result<bool> {
    Ok(revert_db.delete(txn, &id)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Priority, Status};
    use heed::EnvOpenOptions;
    use tempfile::tempdir;

    fn make_task(id: TaskId) -> Task {
        Task {
            id,
            text: format!("task {id}"),
            priority: Priority::B,
            due: Utc::now(),
            est_secs: 1800,
            status: Status::Active,
            created_at: Utc::now(),
            completed_at: None,
            deleted_at: None,
        }
    }

    fn open_dbs(dir: &std::path::Path) -> (heed::Env, RevertDb, MetaDb) {
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(10 * 1024 * 1024)
                .max_dbs(4)
                .open(dir)
                .unwrap()
        };
        let mut txn = env.write_txn().unwrap();
        let revert_db = env.create_database(&mut txn, Some("revert")).unwrap();
        let meta_db = env.create_database(&mut txn, Some("meta")).unwrap();
        txn.commit().unwrap();
        (env, revert_db, meta_db)
    }

    #[test]
    fn push_then_list_returns_entries_in_order() {
        let dir = tempdir().unwrap();
        let (env, rdb, mdb) = open_dbs(dir.path());

        let now = Utc::now();
        let mut txn = env.write_txn().unwrap();
        push(&mut txn, rdb, mdb, RevertOp::Added { id: 1 }, now).unwrap();
        push(&mut txn, rdb, mdb, RevertOp::Deleted { before: make_task(2) }, now).unwrap();
        txn.commit().unwrap();

        let txn = env.read_txn().unwrap();
        let entries = list(&txn, rdb).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, 1);
        assert_eq!(entries[1].0, 2);
    }

    #[test]
    fn prune_keeps_max_history_entries() {
        let dir = tempdir().unwrap();
        let (env, rdb, mdb) = open_dbs(dir.path());

        let now = Utc::now();
        let mut txn = env.write_txn().unwrap();
        for i in 0..(MAX_HISTORY + 5) {
            push(&mut txn, rdb, mdb, RevertOp::Added { id: i as TaskId }, now).unwrap();
        }
        txn.commit().unwrap();

        let txn = env.read_txn().unwrap();
        let entries = list(&txn, rdb).unwrap();
        assert_eq!(entries.len(), MAX_HISTORY);
        // Oldest 5 were dropped; first remaining ID should be 6
        assert_eq!(entries[0].0, 6);
    }

    #[test]
    fn delete_removes_entry() {
        let dir = tempdir().unwrap();
        let (env, rdb, mdb) = open_dbs(dir.path());

        let now = Utc::now();
        let mut txn = env.write_txn().unwrap();
        push(&mut txn, rdb, mdb, RevertOp::Added { id: 1 }, now).unwrap();
        let removed = delete(&mut txn, rdb, 1).unwrap();
        assert!(removed);
        txn.commit().unwrap();

        let txn = env.read_txn().unwrap();
        assert!(list(&txn, rdb).unwrap().is_empty());
    }

    #[test]
    fn summary_added() {
        let op = RevertOp::Added { id: 42 };
        assert_eq!(op.summary(), "added #42");
    }

    #[test]
    fn summary_edited_includes_text() {
        let op = RevertOp::Edited { before: make_task(7) };
        assert!(op.summary().contains("edited #7"));
        assert!(op.summary().contains("task 7"));
    }
}
