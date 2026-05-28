use crate::clock::Clock;
use crate::error::Result;
use crate::model::TaskId;
use crate::store::Store;
use crate::tui::events::PendingChange;
use std::collections::HashMap;

/// Apply any non-edit pending changes that the user toggled in the TUI. Edits are
/// handled inline during the session, so they don't appear here.
pub fn apply(
    pending: &HashMap<TaskId, Vec<PendingChange>>,
    store: &mut Store,
    clock: &dyn Clock,
) -> Result<()> {
    for (id, changes) in pending {
        let id = *id;

        for change in changes
            .iter()
            .filter(|c| matches!(c, PendingChange::SetCategory(_, _)))
        {
            if let PendingChange::SetCategory(_, category) = change {
                let task = store.get_task(id)?;
                let mut updated = task.clone();
                updated.category = *category;
                store.update_task_with_revert(task, updated, clock)?;
            }
        }

        for _ in changes
            .iter()
            .filter(|c| matches!(c, PendingChange::ToggleComplete(_)))
        {
            crate::commands::complete::run(id, store, clock)?;
        }

        for _ in changes
            .iter()
            .filter(|c| matches!(c, PendingChange::ToggleDelete(_)))
        {
            crate::commands::delete::run(id, store, clock)?;
        }
    }
    Ok(())
}
