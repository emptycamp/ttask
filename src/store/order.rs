//! Manual-order shift math: given the active tasks and a (task_id, target_ord)
//! move, compute the new ord for every task affected.
//!
//! The contract: after the shift, the moved task ends up at `target_ord`, and every
//! other active task keeps its relative position to the rest, with their ords
//! re-packed contiguously starting from 1. This avoids gaps growing unboundedly as
//! the user reorders tasks repeatedly.

use crate::model::{Task, TaskId};
use std::collections::HashMap;

/// `active` must already be sorted by current ord ascending. Returns a map from
/// task id to the new ord — every active task appears in the returned map (the
/// caller can skip the no-op writes if `new_ord == old_ord`).
pub fn compute_reorder(active: &[Task], id: TaskId, target_ord: u32) -> HashMap<TaskId, u32> {
    let n = active.len();
    if n == 0 {
        return HashMap::new();
    }
    let target = target_ord.max(1).min(n as u32);

    // Build the new ordering: everyone except the moved task in original order,
    // then insert the moved task at (target - 1).
    let mut order: Vec<TaskId> = active.iter().filter(|t| t.id != id).map(|t| t.id).collect();
    let insert_at = ((target as usize).saturating_sub(1)).min(order.len());
    order.insert(insert_at, id);

    order
        .into_iter()
        .enumerate()
        .map(|(i, tid)| (tid, (i as u32) + 1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status, Task};
    use chrono::Utc;

    fn t(id: u32, ord: u32) -> Task {
        let now = Utc::now();
        Task {
            id,
            text: format!("t{id}"),
            category: Category::B,
            ord,
            est_secs: 1800,
            status: Status::Active,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn move_to_first_position_shifts_others_down() {
        let active = vec![t(1, 1), t(2, 2), t(3, 3)];
        let m = compute_reorder(&active, 3, 1);
        assert_eq!(m[&3], 1);
        assert_eq!(m[&1], 2);
        assert_eq!(m[&2], 3);
    }

    #[test]
    fn move_to_last_position_shifts_others_up() {
        let active = vec![t(1, 1), t(2, 2), t(3, 3)];
        let m = compute_reorder(&active, 1, 3);
        assert_eq!(m[&2], 1);
        assert_eq!(m[&3], 2);
        assert_eq!(m[&1], 3);
    }

    #[test]
    fn move_to_same_position_is_identity() {
        let active = vec![t(1, 1), t(2, 2), t(3, 3)];
        let m = compute_reorder(&active, 2, 2);
        assert_eq!(m[&1], 1);
        assert_eq!(m[&2], 2);
        assert_eq!(m[&3], 3);
    }

    #[test]
    fn target_beyond_end_clamps_to_last() {
        let active = vec![t(1, 1), t(2, 2), t(3, 3)];
        let m = compute_reorder(&active, 1, 99);
        assert_eq!(m[&1], 3);
    }

    #[test]
    fn target_zero_clamps_to_first() {
        let active = vec![t(1, 1), t(2, 2), t(3, 3)];
        let m = compute_reorder(&active, 3, 0);
        assert_eq!(m[&3], 1);
    }

    #[test]
    fn compacts_gaps_in_input_ords() {
        // Input ords have gaps (1, 5, 10); after reorder they are repacked 1..=N.
        let active = vec![t(1, 1), t(2, 5), t(3, 10)];
        let m = compute_reorder(&active, 1, 2);
        assert_eq!(m[&2], 1);
        assert_eq!(m[&1], 2);
        assert_eq!(m[&3], 3);
    }
}
