use crate::error::{Error, Result};
use crate::format::format_est;
use crate::model::{Priority, Task, TaskId};
use crate::store::codec::Bincode;
use chrono::{DateTime, Local, Utc};
use heed::types::U64;
use heed::{Database, RoTxn, RwTxn};
use serde::{Deserialize, Serialize};

pub type RevertDb = Database<U64<heed::byteorder::BigEndian>, Bincode<HistoryEntry>>;
pub type MetaDb = Database<heed::types::Str, U64<heed::byteorder::BigEndian>>;

pub const MAX_HISTORY: usize = 30;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RevertOp {
    /// Task creation. Stores the full task so the history log can render the text,
    /// priority, etc. even after the task is later deleted.
    Added {
        task: Task,
    },
    /// In-place edit. `before` is what the task looked like prior to the change (used
    /// to revert) and `after` is the post-edit state (used to summarize *what* changed
    /// in the history log).
    Edited {
        before: Task,
        after: Task,
    },
    Deleted {
        before: Task,
    },
    Completed {
        before: Task,
    },
}

impl RevertOp {
    /// Compact summary used by `task history list` by default. For edits this prints
    /// only the names of changed fields (e.g. `edited #1: text, p`); use
    /// [`summary_verbose`](Self::summary_verbose) when the caller wants old→new values.
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

    /// Detailed summary used by `task history list -v` and by revert confirmations.
    /// For edits this prints the full per-field diff (`text "old"→"new", p A→B`); for
    /// other variants it matches [`summary`](Self::summary).
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
            // Added/Deleted/Completed already carry the task text in their default
            // summary — verbose mode adds nothing for them.
            other => other.summary(),
        }
    }

    /// The task this operation affected. The cascade uses this so reverting an older
    /// event only pulls in newer events that touch the *same* task — separate tasks
    /// don't share history.
    pub fn task_id(&self) -> TaskId {
        match self {
            RevertOp::Added { task } => task.id,
            RevertOp::Edited { before, .. } => before.id,
            RevertOp::Deleted { before } => before.id,
            RevertOp::Completed { before } => before.id,
        }
    }
}

/// Field-name indicators for the minimal edit summary, e.g. `["text", "p", "est"]`.
fn changed_fields(before: &Task, after: &Task) -> Vec<&'static str> {
    let mut parts: Vec<&'static str> = Vec::new();
    if before.text != after.text {
        parts.push("text");
    }
    if before.priority != after.priority {
        parts.push("p");
    }
    if before.due != after.due {
        parts.push("due");
    }
    if before.est_secs != after.est_secs {
        parts.push("est");
    }
    parts
}

/// Build a compact per-field diff like `text "Buy milk"→"Buy almond milk", p A→B`.
/// Skips fields that didn't change and trims long text so a row stays readable.
fn diff_summary(before: &Task, after: &Task) -> String {
    let mut parts: Vec<String> = Vec::new();
    if before.text != after.text {
        parts.push(format!(
            "text {:?}→{:?}",
            truncate(&before.text, 20),
            truncate(&after.text, 20),
        ));
    }
    if before.priority != after.priority {
        parts.push(format!(
            "p {}→{}",
            priority_letter(before.priority),
            priority_letter(after.priority),
        ));
    }
    if before.due != after.due {
        parts.push(format!(
            "due {}→{}",
            short_due(before.due),
            short_due(after.due),
        ));
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

fn priority_letter(p: Priority) -> char {
    match p {
        Priority::A => 'A',
        Priority::B => 'B',
        Priority::C => 'C',
    }
}

fn short_due(due: DateTime<Utc>) -> String {
    let local: DateTime<Local> = due.into();
    local.format("%b%-d %H:%M").to_string()
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
        // Oldest 5 were dropped; first remaining ID should be 6
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
        // Minimal: just the field name, no old/new values.
        assert_eq!(op.summary(), "edited #7: text");
    }

    #[test]
    fn summary_edited_with_priority_change_uses_p_token() {
        let before = make_task(3);
        let mut after = before.clone();
        after.priority = Priority::A;
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary(), "edited #3: p");
    }

    #[test]
    fn summary_edited_with_due_change_uses_due_token() {
        let before = make_task(2);
        let mut after = before.clone();
        after.due = before.due + chrono::Duration::hours(1);
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary(), "edited #2: due");
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
        after.priority = Priority::C;
        after.est_secs = 60;
        let op = RevertOp::Edited { before, after };
        // Order: text, p, due, est — independent of how the fields were assigned.
        assert_eq!(op.summary(), "edited #9: text, p, est");
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
        assert!(s.contains("task 4"), "got: {s}");
    }

    #[test]
    fn summary_completed_includes_text() {
        let op = RevertOp::Completed {
            before: make_task(6),
        };
        let s = op.summary();
        assert!(s.starts_with("completed #6:"), "got: {s}");
        assert!(s.contains("task 6"), "got: {s}");
    }

    // ── Verbose summary ────────────────────────────────────────────────────────────

    #[test]
    fn summary_verbose_added_matches_default() {
        let op = RevertOp::Added { task: make_task(2) };
        assert_eq!(op.summary_verbose(), op.summary());
    }

    #[test]
    fn summary_verbose_deleted_matches_default() {
        let op = RevertOp::Deleted {
            before: make_task(8),
        };
        assert_eq!(op.summary_verbose(), op.summary());
    }

    #[test]
    fn summary_verbose_completed_matches_default() {
        let op = RevertOp::Completed {
            before: make_task(3),
        };
        assert_eq!(op.summary_verbose(), op.summary());
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
    fn summary_verbose_edited_includes_priority_letters() {
        let before = make_task(1);
        let mut after = before.clone();
        after.priority = Priority::A;
        let op = RevertOp::Edited { before, after };
        assert!(op.summary_verbose().contains("p B→A"));
    }

    #[test]
    fn summary_verbose_edited_includes_est_units() {
        let before = make_task(1);
        let mut after = before.clone();
        after.est_secs = 7200;
        let op = RevertOp::Edited { before, after };
        assert!(op.summary_verbose().contains("est 30m→2h"));
    }

    #[test]
    fn summary_verbose_edited_with_no_change_is_just_the_id() {
        let before = make_task(11);
        let after = before.clone();
        let op = RevertOp::Edited { before, after };
        assert_eq!(op.summary_verbose(), "edited #11");
    }

    #[test]
    fn summary_minimal_for_edits_does_not_include_arrows() {
        // The whole point of the minimal form is that it omits old→new arrows.
        let before = make_task(1);
        let mut after = before.clone();
        after.text = "x".into();
        after.priority = Priority::A;
        let s = RevertOp::Edited { before, after }.summary();
        assert!(
            !s.contains('→'),
            "minimal summary must not include arrows: {s}"
        );
        assert!(
            !s.contains('"'),
            "minimal summary must not quote old/new values: {s}"
        );
    }
}
