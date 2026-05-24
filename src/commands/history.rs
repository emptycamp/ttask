use crate::confirm::Prompt;
use crate::error::{Error, Result};
use crate::format::{format_history, RenderOptions};
use crate::store::revert::{HistoryEntry, RevertOp};
use crate::store::Store;

pub fn list(store: &Store, opts: &RenderOptions, verbose: bool) -> Result<String> {
    let entries = store.history()?;
    Ok(format_history(&entries, opts, verbose))
}

/// Result of a cascade revert: every event that was rolled back, newest first.
pub type RevertSummary = Vec<(u64, String)>;

/// Revert event `from_id` and every event newer than it, in newest-first order.
///
/// History events are layered: a later event was applied on top of earlier state. To
/// undo an older event cleanly we have to first undo every newer event, otherwise we'd
/// be reverting a task to a state it was never in. The function asks for confirmation
/// once (showing the full cascade), then applies the reverts.
pub fn revert(
    from_id: u64,
    yes: bool,
    store: &mut Store,
    prompt: &dyn Prompt,
) -> Result<RevertSummary> {
    let cascade = collect_cascade(store, from_id)?;

    if !yes && !prompt.confirm(&confirm_message(&cascade))? {
        return Err(Error::Cancelled);
    }

    let mut summaries = Vec::with_capacity(cascade.len());
    for (id, entry) in &cascade {
        // The revert prompt and the "Reverted event #N: ..." message both want full
        // diffs — the user is acting on / acknowledging the change, so the extra
        // detail is worth the line length.
        let summary = entry.op.summary_verbose();
        store.history_revert(*id)?;
        summaries.push((*id, summary));
    }
    Ok(summaries)
}

/// Collect the cascade for `from_id`: the target event plus every newer event that
/// touches the *same task incarnation*. Newest-first. Returns an error if the target
/// id doesn't exist.
///
/// Tasks are independent — an edit to task #2 doesn't depend on an unrelated add of
/// task #5, so reverting the add shouldn't drag the edit along. Task IDs are also
/// recycled (`next_id` fills the lowest gap), so we treat each `Added { id }` event
/// as the start of a new incarnation: when scanning newer events for the cascade, we
/// stop as soon as we see another `Added` for the same task id, since anything past
/// that point belongs to a different incarnation.
pub fn collect_cascade(store: &Store, from_id: u64) -> Result<Vec<(u64, HistoryEntry)>> {
    let mut entries = store.history()?;
    // history() returns ascending by id; make sure of it so the loop below sees
    // events from oldest to newest.
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let target_task_id = entries
        .iter()
        .find(|(id, _)| *id == from_id)
        .map(|(_, e)| e.op.task_id())
        .ok_or(Error::HistoryNotFound(from_id))?;

    let mut collected: Vec<(u64, HistoryEntry)> = Vec::new();
    for (id, entry) in entries.into_iter() {
        if id < from_id {
            continue;
        }
        if entry.op.task_id() != target_task_id {
            continue;
        }
        if id != from_id {
            if let RevertOp::Added { task: ref added } = entry.op {
                if added.id == target_task_id {
                    // New incarnation starts here — stop collecting.
                    break;
                }
            }
        }
        collected.push((id, entry));
    }
    collected.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(collected)
}

fn confirm_message(cascade: &[(u64, HistoryEntry)]) -> String {
    if cascade.len() == 1 {
        let (id, e) = &cascade[0];
        return format!("Revert event #{id} ({})?", e.op.summary_verbose());
    }
    // All entries in the cascade share the same task id by construction.
    let task_id = cascade.first().map(|(_, e)| e.op.task_id()).unwrap_or(0);
    let mut msg = format!(
        "Reverting an older event rolls back every newer event on the same task.\n\
         This will revert {} events on task #{task_id} (newest first):\n",
        cascade.len()
    );
    for (id, e) in cascade {
        msg.push_str(&format!("  #{id}  {}\n", e.op.summary_verbose()));
    }
    msg.push_str("Continue?");
    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::confirm::AutoConfirm;
    use crate::model::{Priority, Status, Task};
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    fn make_clock() -> FakeClock {
        FakeClock::new(Utc.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap())
    }

    fn make_task(id: u32) -> Task {
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

    #[test]
    fn revert_unknown_event_id_errors() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        assert!(matches!(
            revert(999, true, &mut store, &AutoConfirm),
            Err(Error::HistoryNotFound(999))
        ));
    }

    #[test]
    fn revert_latest_only_reverts_one_event() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        store.add_task_with_revert(make_task(1), &clock).unwrap();
        store.add_task_with_revert(make_task(2), &clock).unwrap();

        let entries = store.history().unwrap();
        let latest_id = entries.iter().map(|(id, _)| *id).max().unwrap();
        let result = revert(latest_id, true, &mut store, &AutoConfirm).unwrap();
        assert_eq!(result.len(), 1);
        // The newer task is gone; the older one survives.
        assert!(store.get_task(2).is_err());
        assert!(store.get_task(1).is_ok());
    }

    #[test]
    fn revert_cascade_only_includes_same_task_events() {
        // Three independent tasks — reverting the oldest event should NOT pull in
        // the newer events on different tasks.
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();
        store.add_task_with_revert(make_task(1), &clock).unwrap();
        store.add_task_with_revert(make_task(2), &clock).unwrap();
        store.add_task_with_revert(make_task(3), &clock).unwrap();

        let entries = store.history().unwrap();
        let oldest_id = entries.iter().map(|(id, _)| *id).min().unwrap();

        let result = revert(oldest_id, true, &mut store, &AutoConfirm).unwrap();
        assert_eq!(
            result.len(),
            1,
            "cascade should be scoped to the target task"
        );
        assert!(store.get_task(1).is_err());
        // Tasks #2 and #3 untouched — they're independent.
        assert!(store.get_task(2).is_ok());
        assert!(store.get_task(3).is_ok());
    }

    #[test]
    fn revert_older_event_cascades_through_same_task_only() {
        // Task #1 gets added, edited, completed. Task #2 just gets added. Reverting
        // the add-of-#1 cascades through #1's full history but skips #2.
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();

        let t1 = make_task(1);
        store.add_task_with_revert(t1.clone(), &clock).unwrap();
        let t2 = make_task(2);
        store.add_task_with_revert(t2.clone(), &clock).unwrap();
        // Edit task #1
        let mut t1_edited = t1.clone();
        t1_edited.text = "renamed".into();
        store
            .update_task_with_revert(t1.clone(), t1_edited.clone(), &clock)
            .unwrap();
        // Complete task #1
        let mut t1_done = t1_edited.clone();
        t1_done.status = crate::model::Status::Completed;
        store
            .complete_task_with_revert(t1_edited.clone(), t1_done, &clock)
            .unwrap();

        let entries = store.history().unwrap();
        // Find the add-of-#1 event.
        let target_id = entries
            .iter()
            .find(|(_, e)| matches!(&e.op, crate::store::revert::RevertOp::Added { task } if task.id == 1))
            .map(|(id, _)| *id)
            .expect("add-of-#1 event should exist");

        let result = revert(target_id, true, &mut store, &AutoConfirm).unwrap();
        // Cascade: complete-#1, edit-#1, add-#1 (3 events on task #1). Add-#2 untouched.
        assert_eq!(result.len(), 3);
        assert!(store.get_task(1).is_err(), "task #1 should be fully gone");
        assert!(
            store.get_task(2).is_ok(),
            "task #2 was never part of the cascade"
        );
    }

    #[test]
    fn confirm_message_single_event_is_concise() {
        let entry = HistoryEntry {
            op: crate::store::revert::RevertOp::Added { task: make_task(5) },
            timestamp: Utc::now(),
        };
        let msg = confirm_message(&[(7, entry)]);
        assert!(msg.contains("#7"));
        assert!(msg.contains("added #5"));
    }

    #[test]
    fn cascade_stops_at_new_incarnation_of_reused_task_id() {
        // M4: task IDs are recycled when an earlier task is removed. If we revert the
        // very first event for task #1 (an Added), we must NOT cascade through events
        // belonging to a later, unrelated task that happened to get the same id #1.
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let clock = make_clock();

        // First incarnation: add task #1, edit it.
        let t1 = make_task(1);
        store.add_task_with_revert(t1.clone(), &clock).unwrap();
        let mut t1_edited = t1.clone();
        t1_edited.text = "first incarnation edit".into();
        store
            .update_task_with_revert(t1.clone(), t1_edited.clone(), &clock)
            .unwrap();

        // Simulate the first incarnation being gone (hard-delete bypasses history).
        store.hard_delete(1).unwrap();

        // Second incarnation: a new task added that happens to reuse id #1.
        store.add_task_with_revert(make_task(1), &clock).unwrap();
        let new_t1 = store.get_task(1).unwrap();
        let mut new_t1_edited = new_t1.clone();
        new_t1_edited.text = "second incarnation edit".into();
        store
            .update_task_with_revert(new_t1, new_t1_edited, &clock)
            .unwrap();

        // Find the FIRST Added{id:1} event — that's the older incarnation.
        let entries = store.history().unwrap();
        let first_added_id = entries
            .iter()
            .find(|(_, e)| matches!(&e.op, crate::store::revert::RevertOp::Added { task } if task.id == 1))
            .map(|(id, _)| *id)
            .expect("first add-of-#1 should be in history");

        let cascade = collect_cascade(&store, first_added_id).unwrap();
        // Expected: target (first Added) + first incarnation's edit. The second
        // Added and its edit must NOT be included.
        assert_eq!(
            cascade.len(),
            2,
            "cascade should only include the first incarnation, got: {cascade:?}",
        );
    }

    #[test]
    fn confirm_message_cascade_lists_every_event_and_names_task() {
        // All entries share task #1 — the cascade scope.
        let now = Utc::now();
        let entries = vec![
            (
                9,
                HistoryEntry {
                    op: crate::store::revert::RevertOp::Completed {
                        before: make_task(1),
                    },
                    timestamp: now,
                },
            ),
            (
                8,
                HistoryEntry {
                    op: crate::store::revert::RevertOp::Edited {
                        before: make_task(1),
                        after: make_task(1),
                    },
                    timestamp: now,
                },
            ),
            (
                7,
                HistoryEntry {
                    op: crate::store::revert::RevertOp::Added { task: make_task(1) },
                    timestamp: now,
                },
            ),
        ];
        let msg = confirm_message(&entries);
        assert!(msg.contains("3 events"));
        assert!(msg.contains("#9"));
        assert!(msg.contains("#8"));
        assert!(msg.contains("#7"));
        assert!(msg.contains("same task"));
        assert!(msg.contains("task #1"));
    }
}
