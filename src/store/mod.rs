pub mod codec;
pub mod gc;
pub mod revert;
pub mod tasks;

use crate::clock::Clock;
use crate::error::{Error, Result};
use crate::model::{Task, TaskId};
use crate::store::revert::{HistoryEntry, MetaDb, RevertDb, RevertOp};
use crate::store::tasks::TasksDb;
use directories::ProjectDirs;
use heed::{Env, EnvOpenOptions};
use std::path::{Path, PathBuf};

pub struct Store {
    env: Env,
    tasks_db: TasksDb,
    revert_db: RevertDb,
    meta_db: MetaDb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearStats {
    pub tasks_cleared: u32,
    pub events_cleared: u32,
}

/// Which kind of history event to record for a `mutate_task` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutateKind {
    Edit,
    Delete,
    Complete,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path).map_err(Error::Io)?;
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(64 * 1024 * 1024)
                .max_dbs(4)
                .open(path)
                .map_err(Error::Db)?
        };

        let mut txn = env.write_txn()?;
        let tasks_db = env.create_database(&mut txn, Some("tasks"))?;
        // Named "history_v2" to keep the new schema separate from any legacy
        // RevertOp-encoded entries left over from earlier versions. Old data in
        // "revert" and "history" is left in place but ignored — bincode lacks the
        // tolerance for added/changed enum-variant fields, so re-reading historical
        // entries would just fail to decode.
        let revert_db = env.create_database(&mut txn, Some("history_v2"))?;
        let meta_db = env.create_database(&mut txn, Some("meta"))?;
        txn.commit()?;

        Ok(Self {
            env,
            tasks_db,
            revert_db,
            meta_db,
        })
    }

    pub fn default_path(test_mode: bool) -> PathBuf {
        if let Ok(dir) = std::env::var("TASK_DATA_DIR") {
            return PathBuf::from(dir).join("db");
        }
        let name = if test_mode { "task-test" } else { "task" };
        ProjectDirs::from("", "", name)
            .expect("could not determine data directory")
            .data_dir()
            .join("db")
    }

    pub fn all_tasks(&self) -> Result<Vec<Task>> {
        let txn = self.env.read_txn()?;
        tasks::all(&txn, self.tasks_db)
    }

    pub fn get_task(&self, id: TaskId) -> Result<Task> {
        let txn = self.env.read_txn()?;
        tasks::get(&txn, self.tasks_db, id)
    }

    pub fn add_task(&mut self, task: Task) -> Result<Task> {
        let mut txn = self.env.write_txn()?;
        tasks::put(&mut txn, self.tasks_db, &task)?;
        txn.commit()?;
        Ok(task)
    }

    pub fn add_task_with_revert(&mut self, task: Task, clock: &dyn Clock) -> Result<Task> {
        let mut txn = self.env.write_txn()?;
        tasks::put(&mut txn, self.tasks_db, &task)?;
        revert::push(
            &mut txn,
            self.revert_db,
            self.meta_db,
            RevertOp::Added { task: task.clone() },
            clock.now(),
        )?;
        txn.commit()?;
        Ok(task)
    }

    /// Allocate a new task ID and insert the task in a single write transaction.
    ///
    /// `build` is invoked with the next free ID inside the txn so two concurrent
    /// `task add` invocations don't pick the same ID and clobber each other (the lmdb
    /// writer is single-threaded, so the second one sees the first one's commit).
    pub fn add_task_atomic<F>(&mut self, build: F, clock: &dyn Clock) -> Result<Task>
    where
        F: FnOnce(TaskId) -> Task,
    {
        let mut txn = self.env.write_txn()?;
        let id = tasks::next_id(&txn, self.tasks_db)?;
        let task = build(id);
        tasks::put(&mut txn, self.tasks_db, &task)?;
        revert::push(
            &mut txn,
            self.revert_db,
            self.meta_db,
            RevertOp::Added { task: task.clone() },
            clock.now(),
        )?;
        txn.commit()?;
        Ok(task)
    }

    pub fn next_id(&self) -> Result<TaskId> {
        let txn = self.env.read_txn()?;
        tasks::next_id(&txn, self.tasks_db)
    }

    pub fn update_task(&mut self, task: Task) -> Result<()> {
        let mut txn = self.env.write_txn()?;
        tasks::put(&mut txn, self.tasks_db, &task)?;
        txn.commit()?;
        Ok(())
    }

    pub fn update_task_with_revert(
        &mut self,
        before: Task,
        after: Task,
        clock: &dyn Clock,
    ) -> Result<()> {
        let mut txn = self.env.write_txn()?;
        tasks::put(&mut txn, self.tasks_db, &after)?;
        revert::push(
            &mut txn,
            self.revert_db,
            self.meta_db,
            RevertOp::Edited {
                before,
                after: after.clone(),
            },
            clock.now(),
        )?;
        txn.commit()?;
        Ok(())
    }

    pub fn soft_delete_task_with_revert(
        &mut self,
        before: Task,
        after: Task,
        clock: &dyn Clock,
    ) -> Result<()> {
        let mut txn = self.env.write_txn()?;
        tasks::put(&mut txn, self.tasks_db, &after)?;
        revert::push(
            &mut txn,
            self.revert_db,
            self.meta_db,
            RevertOp::Deleted { before },
            clock.now(),
        )?;
        txn.commit()?;
        Ok(())
    }

    pub fn complete_task_with_revert(
        &mut self,
        before: Task,
        after: Task,
        clock: &dyn Clock,
    ) -> Result<()> {
        let mut txn = self.env.write_txn()?;
        tasks::put(&mut txn, self.tasks_db, &after)?;
        revert::push(
            &mut txn,
            self.revert_db,
            self.meta_db,
            RevertOp::Completed { before },
            clock.now(),
        )?;
        txn.commit()?;
        Ok(())
    }

    /// Atomic read-modify-write. Reads the current task inside a write transaction,
    /// invokes `modify` to produce the new state, then writes the new state and pushes
    /// a history event — all in the same transaction.
    ///
    /// This prevents lost updates from concurrent edits: each writer sees the latest
    /// committed state, so the recorded `before` and the persisted `after` are always
    /// consistent.
    pub fn mutate_task<F>(
        &mut self,
        id: TaskId,
        kind: MutateKind,
        modify: F,
        clock: &dyn Clock,
    ) -> Result<Task>
    where
        F: FnOnce(&Task) -> Result<Task>,
    {
        let mut txn = self.env.write_txn()?;
        let before = tasks::get(&txn, self.tasks_db, id)?;
        let after = modify(&before)?;
        if after == before {
            return Ok(after);
        }
        tasks::put(&mut txn, self.tasks_db, &after)?;
        let op = match kind {
            MutateKind::Edit => RevertOp::Edited {
                before,
                after: after.clone(),
            },
            MutateKind::Delete => RevertOp::Deleted { before },
            MutateKind::Complete => RevertOp::Completed { before },
        };
        revert::push(&mut txn, self.revert_db, self.meta_db, op, clock.now())?;
        txn.commit()?;
        Ok(after)
    }

    pub fn hard_delete(&mut self, id: TaskId) -> Result<()> {
        let mut txn = self.env.write_txn()?;
        tasks::delete(&mut txn, self.tasks_db, id)?;
        txn.commit()?;
        Ok(())
    }

    /// Wipe every task and every history event. Irreversible — the caller is
    /// responsible for getting confirmation first.
    pub fn clear_all(&mut self) -> Result<ClearStats> {
        let mut txn = self.env.write_txn()?;
        let tasks_cleared = self.tasks_db.len(&txn)? as u32;
        let events_cleared = self.revert_db.len(&txn)? as u32;
        self.tasks_db.clear(&mut txn)?;
        self.revert_db.clear(&mut txn)?;
        self.meta_db.clear(&mut txn)?;
        txn.commit()?;
        Ok(ClearStats {
            tasks_cleared,
            events_cleared,
        })
    }

    pub fn history(&self) -> Result<Vec<(u64, HistoryEntry)>> {
        let txn = self.env.read_txn()?;
        revert::list(&txn, self.revert_db)
    }

    pub fn history_get(&self, id: u64) -> Result<Option<HistoryEntry>> {
        let txn = self.env.read_txn()?;
        revert::get(&txn, self.revert_db, id)
    }

    pub fn history_revert(&mut self, id: u64) -> Result<()> {
        let entry = self.history_get(id)?.ok_or(Error::HistoryNotFound(id))?;
        let op = entry.op.clone();
        self.apply_revert_op(op)?;
        let mut txn = self.env.write_txn()?;
        revert::delete(&mut txn, self.revert_db, id)?;
        txn.commit()?;
        Ok(())
    }

    fn apply_revert_op(&mut self, op: RevertOp) -> Result<()> {
        match op {
            RevertOp::Added { task } => {
                self.hard_delete(task.id)?;
            }
            RevertOp::Edited { before, .. } => {
                self.update_task(before)?;
            }
            RevertOp::Deleted { mut before } => {
                before.status = crate::model::Status::Active;
                before.deleted_at = None;
                self.update_task(before)?;
            }
            RevertOp::Completed { mut before } => {
                before.status = crate::model::Status::Active;
                before.completed_at = None;
                self.update_task(before)?;
            }
        }
        Ok(())
    }
}
