use crate::error::{Error, Result};
use crate::format::format_est;
use crate::model::{Category, Task, TaskId};
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
    Added { task: Task },
    Edited { before: Task, after: Task },
    Deleted { before: Task },
    Completed { before: Task },
}

impl RevertOp {
    pub fn summary(&self) -> String {
        match self {
            RevertOp::Added { task } => {
                format!("added #{}: {}", task.id, truncate(&task.text, 30))
            }
            RevertOp::Edited { before, after } => {
                let fields = changed_fields(before, after);
                if fields.is_empty() {
                    format!("edited #{}", before.id)
                } else {
                    format!("edited #{}: {}", before.id, fields.join(", "))
                }
            }
            RevertOp::Deleted { before } => {
                format!("deleted #{}: {}", before.id, truncate(&before.text, 30))
            }
            RevertOp::Completed { before } => {
                format!("completed #{}: {}", before.id, truncate(&before.text, 30))
            }
        }
    }

    pub fn summary_verbose(&self) -> String {
        match self {
            RevertOp::Edited { before, after } => {
                let diff = diff_summary(before, after);
                if diff.is_empty() {
                    format!("edited #{}", before.id)
                } else {
                    format!("edited #{}: {diff}", before.id)
                }
            }
            other => other.summary(),
        }
    }

    pub fn task_id(&self) -> TaskId {
        match self {
            RevertOp::Added { task } => task.id,
            RevertOp::Edited { before, .. } => before.id,
            RevertOp::Deleted { before } => before.id,
            RevertOp::Completed { before } => before.id,
        }
    }
}

fn changed_fields(before: &Task, after: &Task) -> Vec<&'static str> {
    let mut parts: Vec<&'static str> = Vec::new();
    if before.text != after.text {
        parts.push("text");
    }
    if before.category != after.category {
        parts.push("cat");
    }
    if before.ord != after.ord {
        parts.push("ord");
    }
    if before.est_secs != after.est_secs {
        parts.push("est");
    }
    parts
}

fn diff_summary(before: &Task, after: &Task) -> String {
    let mut parts: Vec<String> = Vec::new();
    if before.text != after.text {
        parts.push(format!(
            "text {:?}→{:?}",
            truncate(&before.text, 20),
            truncate(&after.text, 20),
        ));
    }
    if before.category != after.category {
        parts.push(format!(
            "cat {}→{}",
            category_letter(before.category),
            category_letter(after.category),
        ));
    }
    if before.ord != after.ord {
        parts.push(format!("ord {}→{}", before.ord, after.ord));
    }
    if before.est_secs != after.est_secs {
        parts.push(format!(
            "est {}→{}",
            format_est(before.est_secs),
            format_est(after.est_secs),
        ));
    }
    parts.join(", ")
}

fn category_letter(p: Category) -> char {
    match p {
        Category::A => 'A',
        Category::B => 'B',
        Category::C => 'C',
    }
}

fn truncate(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        s.to_string()
    } else {
        format!(
            "{}...",
            &chars[..width.saturating_sub(3)].iter().collect::<String>()
        )
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
    revert_db.iter(txn)?.map(|r| r.map_err(Error::Db)).collect()
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
    use crate::model::{Category, Status};
    use heed::EnvOpenOptions;
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
        push(
            &mut txn,
            rdb,
            mdb,
            RevertOp::Added { task: make_task(1) },
            now,
        )
        .unwrap();
        push(
            &mut txn,
            rdb,
            mdb,
            RevertOp::Deleted {
                before: make_task(2),
            },
            now,
        )
        .unwrap();
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
            push(
                &mut txn,
                rdb,
                mdb,
                RevertOp::Added {
                    task: make_task(i as TaskId),
                },
                now,
            )
            .unwrap();
        }
        txn.commit().unwrap();

        let txn = env.read_txn().unwrap();
        let entries = list(&txn, rdb).unwrap();
        assert_eq!(entries.len(), MAX_HISTORY);
        assert_eq!(entries[0].0, 6);
    }

    #[test]
    fn delete_removes_entry() {
        let dir = tempdir().unwrap();
        let (env, rdb, mdb) = open_dbs(dir.path());

        let now = Utc::now();
        let mut txn = env.write_txn().unwrap();
        push(
            &mut txn,
            rdb,
            mdb,
            RevertOp::Added { task: make_task(1) },
            now,
        )
        .unwrap();
        let removed = delete(&mut txn, rdb, 1).unwrap();
        assert!(removed);
        txn.commit().unwrap();

        let txn = env.read_txn().unwrap();
        assert!(list(&txn, rdb).unwrap().is_empty());
    }

    #[test]
    fn summary_added_includes_text() {
        let op = RevertOp::Added {
            task: make_task(42),
        };
        let s = op.summary();
        assert!(s.starts_with("added #42:"), "got: {s}");
        assert!(s.contains("task 42"), "got: {s}");
    }

    #[test]
    fn summary_edited_with_single_field_shows_only_field_name() {
        let before = make_task(7);
        let mut after = before.clone();
        after.text = "renamed task".into();
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary(), "edited #7: text");
    }

    #[test]
    fn summary_edited_with_category_change_uses_cat_token() {
        let before = make_task(3);
        let mut after = before.clone();
        after.category = Category::A;
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary(), "edited #3: cat");
    }

    #[test]
    fn summary_edited_with_ord_change_uses_ord_token() {
        let before = make_task(2);
        let mut after = before.clone();
        after.ord = before.ord + 5;
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary(), "edited #2: ord");
    }

    #[test]
    fn summary_edited_with_est_change_uses_est_token() {
        let before = make_task(5);
        let mut after = before.clone();
        after.est_secs = 7200;
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary(), "edited #5: est");
    }

    #[test]
    fn summary_edited_lists_all_changed_field_tokens_in_canonical_order() {
        let before = make_task(9);
        let mut after = before.clone();
        after.text = "new text".into();
        after.category = Category::C;
        after.est_secs = 60;
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary(), "edited #9: text, cat, est");
    }

    #[test]
    fn summary_edited_with_no_changes_is_just_the_id() {
        let before = make_task(11);
        let after = before.clone();
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary(), "edited #11");
    }

    #[test]
    fn summary_deleted_includes_text() {
        let op = RevertOp::Deleted {
            before: make_task(4),
        };
        let s = op.summary();
        assert!(s.starts_with("deleted #4:"), "got: {s}");
    }

    #[test]
    fn summary_completed_includes_text() {
        let op = RevertOp::Completed {
            before: make_task(6),
        };
        let s = op.summary();
        assert!(s.starts_with("completed #6:"), "got: {s}");
    }

    #[test]
    fn summary_verbose_edited_includes_old_and_new_text() {
        let before = make_task(1);
        let mut after = before.clone();
        after.text = "renamed".into();
        let op = RevertOp::Edited { before, after };
        let v = op.summary_verbose();
        assert!(v.contains("text \"task 1\"→\"renamed\""), "got: {v}");
    }

    #[test]
    fn summary_verbose_edited_includes_category_letters() {
        let before = make_task(1);
        let mut after = before.clone();
        after.category = Category::A;
        let op = RevertOp::Edited { before, after };
        assert!(op.summary_verbose().contains("cat B→A"));
    }

    #[test]
    fn summary_verbose_edited_includes_ord_change() {
        let before = make_task(1);
        let mut after = before.clone();
        after.ord = before.ord + 4;
        let op = RevertOp::Edited { before, after };
        let v = op.summary_verbose();
        assert!(v.contains("ord"), "got: {v}");
    }

    #[test]
    fn summary_minimal_for_edits_does_not_include_arrows() {
        let before = make_task(1);
        let mut after = before.clone();
        after.text = "x".into();
        after.category = Category::A;
        let s = RevertOp::Edited { before, after }.summary();
        assert!(!s.contains('→'));
        assert!(!s.contains('"'));
    }
}
