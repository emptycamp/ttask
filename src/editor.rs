use crate::error::Result;
use crate::model::Task;
use crate::yaml::from_yaml;

/// A function the editor calls to persist an in-progress version of the task.
///
/// Returns the persisted task — for new tasks this includes the assigned ID. The
/// returned value becomes the new baseline for subsequent saves in the same session.
pub type Saver<'a> = dyn FnMut(Task) -> Result<Task> + 'a;

pub trait TaskEditor {
    /// Open an interactive edit session for `task`. The editor may invoke `save` zero or
    /// more times during the session (`:w`, `:wq`, …). The function returns when the
    /// user requests to leave the editor; any persistence happened through `save`.
    fn edit(&self, task: &Task, save: &mut Saver<'_>) -> Result<()>;
}

/// Production editor: opens a built-in terminal form (no external program).
pub struct BuiltinEditor;

impl TaskEditor for BuiltinEditor {
    fn edit(&self, task: &Task, save: &mut Saver<'_>) -> Result<()> {
        if let Ok(yaml) = std::env::var("TASK_EDIT_YAML") {
            let updated = from_yaml(&yaml, task)?;
            save(updated)?;
            return Ok(());
        }
        if std::env::var("TASK_EDIT_CANCEL").is_ok() {
            return Ok(());
        }
        crate::form_editor::run(task, save)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status};
    use chrono::Utc;

    fn make_task() -> Task {
        Task {
            id: 1,
            text: "old text".to_string(),
            category: Category::B,
            ord: 1,
            est_secs: 1800,
            status: Status::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn builtin_editor_yaml_env_var_calls_save_with_parsed_task() {
        let task = make_task();
        std::env::set_var(
            "TASK_EDIT_YAML",
            "text: new text\ncategory: A\nord: 1\nest: 15m\n",
        );
        let mut saved: Option<Task> = None;
        BuiltinEditor
            .edit(&task, &mut |proposed| {
                saved = Some(proposed.clone());
                Ok(proposed)
            })
            .unwrap();
        std::env::remove_var("TASK_EDIT_YAML");
        let saved = saved.expect("save closure should have been called");
        assert_eq!(saved.text, "new text");
        assert_eq!(saved.category, Category::A);
    }

    #[test]
    fn builtin_editor_cancel_env_var_does_not_call_save() {
        let task = make_task();
        std::env::set_var("TASK_EDIT_CANCEL", "1");
        let mut called = false;
        BuiltinEditor
            .edit(&task, &mut |t| {
                called = true;
                Ok(t)
            })
            .unwrap();
        std::env::remove_var("TASK_EDIT_CANCEL");
        assert!(!called);
    }
}
