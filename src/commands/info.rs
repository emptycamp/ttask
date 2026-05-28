use crate::error::Result;
use crate::format::{format_info, RenderOptions};
use crate::model::TaskId;
use crate::store::Store;

pub fn run(id: TaskId, store: &Store, opts: &RenderOptions) -> Result<String> {
    let task = store.get_task(id)?;
    Ok(format_info(&task, opts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status, Task};
    use chrono::Utc;
    use tempfile::tempdir;

    fn make_task(id: u32) -> Task {
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
    fn info_returns_formatted_task() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1)).unwrap();
        let opts = RenderOptions::no_color();
        let output = run(1, &store, &opts).unwrap();
        assert!(output.contains("task 1"));
        assert!(output.contains("Task #1"));
    }

    #[test]
    fn info_nonexistent_task_returns_error() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let opts = RenderOptions::no_color();
        assert!(run(99, &store, &opts).is_err());
    }
}
