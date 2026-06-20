use crate::model::{Category, TaskId};
use crate::tui::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What the run-loop should do in response to a key. Every variant that names a
/// task triggers an **immediate** store mutation (applied right away, not on
/// quit) that the run-loop records on the undo stack so `u` / `r` can reverse it.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Continue,
    Quit,
    EditTask(TaskId),
    AddTask,
    /// User typed a 1-9 digit: move the cursor task to that 1-based position
    /// within its category.
    ReorderCursor(u32),
    SetCategory(TaskId, Category),
    Complete(TaskId),
    Delete(TaskId),
    /// Open a link from the task's text (same flow as `ttask open <id>`).
    OpenLink(TaskId),
    Undo,
    Redo,
}

pub fn handle(app: &mut App, key: KeyEvent) -> Action {
    if app.search_input.is_some() {
        return handle_search_input(app, key);
    }
    handle_normal(app, key)
}

fn handle_normal(app: &mut App, key: KeyEvent) -> Action {
    let visible = app.filtered_tasks();
    let visible_len = visible.len();
    let cursor_id = visible.get(app.cursor).map(|t| t.id);
    drop(visible);

    match (key.code, key.modifiers) {
        (KeyCode::Up, _) => {
            app.cursor = app.cursor.saturating_sub(1);
        }
        (KeyCode::Down, _) => {
            app.cursor = (app.cursor + 1).min(visible_len.saturating_sub(1));
        }
        (KeyCode::Enter, _) => {
            if let Some(id) = cursor_id {
                return Action::EditTask(id);
            }
        }
        (KeyCode::Char('/'), KeyModifiers::NONE) => {
            app.search_input = Some(app.search_filter.clone());
            app.clamp_cursor();
        }
        (KeyCode::Char('a'), KeyModifiers::NONE) => {
            return Action::AddTask;
        }
        (KeyCode::Char('e'), KeyModifiers::NONE) => {
            if let Some(id) = cursor_id {
                return Action::EditTask(id);
            }
        }
        (KeyCode::Char('o'), KeyModifiers::NONE) => {
            if let Some(id) = cursor_id {
                return Action::OpenLink(id);
            }
        }
        (KeyCode::Char('c'), KeyModifiers::NONE) => {
            if let Some(id) = cursor_id {
                return Action::Complete(id);
            }
        }
        (KeyCode::Char('d'), KeyModifiers::NONE) => {
            if let Some(id) = cursor_id {
                return Action::Delete(id);
            }
        }
        (KeyCode::Char('u'), KeyModifiers::NONE) => {
            return Action::Undo;
        }
        (KeyCode::Char('r'), KeyModifiers::NONE) => {
            return Action::Redo;
        }
        (KeyCode::Char('A'), KeyModifiers::SHIFT) => {
            if let Some(id) = cursor_id {
                return Action::SetCategory(id, Category::A);
            }
        }
        (KeyCode::Char('B'), KeyModifiers::SHIFT) => {
            if let Some(id) = cursor_id {
                return Action::SetCategory(id, Category::B);
            }
        }
        (KeyCode::Char('C'), KeyModifiers::SHIFT) => {
            if let Some(id) = cursor_id {
                return Action::SetCategory(id, Category::C);
            }
        }
        // 1-9 reorders the cursor task to that 1-based position within its category.
        (KeyCode::Char(c), KeyModifiers::NONE)
            if cursor_id.is_some() && c.is_ascii_digit() && c != '0' =>
        {
            let n = (c as u32) - ('0' as u32);
            return Action::ReorderCursor(n);
        }
        (KeyCode::Esc, _) => {
            if !app.search_filter.is_empty() {
                app.search_filter.clear();
                app.clamp_cursor();
            } else {
                return Action::Quit;
            }
        }
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Action::Quit;
        }
        _ => {}
    }
    Action::Continue
}

fn handle_search_input(app: &mut App, key: KeyEvent) -> Action {
    match (key.code, key.modifiers) {
        (KeyCode::Enter, _) => {
            if let Some(buf) = app.search_input.take() {
                app.search_filter = buf;
            }
            app.clamp_cursor();
        }
        (KeyCode::Esc, _) => {
            app.search_input = None;
            app.clamp_cursor();
        }
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            return Action::Quit;
        }
        (KeyCode::Backspace, _) => {
            if let Some(buf) = app.search_input.as_mut() {
                buf.pop();
            }
            app.clamp_cursor();
        }
        (KeyCode::Up, _) => {
            app.cursor = app.cursor.saturating_sub(1);
        }
        (KeyCode::Down, _) => {
            let len = app.filtered_tasks().len();
            if app.cursor + 1 < len {
                app.cursor += 1;
            }
        }
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
            if let Some(buf) = app.search_input.as_mut() {
                buf.push(c);
            }
            app.cursor = 0;
        }
        _ => {}
    }
    Action::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status, Task};
    use crate::tui::App;
    use chrono::Utc;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

    fn make_app() -> App {
        App::new(vec![make_task(1), make_task(2)])
    }

    fn make_app_with_text(texts: &[&str]) -> App {
        let tasks: Vec<Task> = texts
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut t = make_task((i + 1) as u32);
                t.text = s.to_string();
                t
            })
            .collect();
        App::new(tasks)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    #[test]
    fn up_moves_cursor_up_clamped() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Up));
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn down_moves_cursor_down() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Down));
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn down_clamped_at_last_item() {
        let mut app = make_app();
        app.cursor = 1;
        handle(&mut app, key(KeyCode::Down));
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn a_returns_add_action() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('a')));
        assert_eq!(action, Action::AddTask);
    }

    #[test]
    fn c_returns_immediate_complete_action() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('c')));
        assert_eq!(action, Action::Complete(1));
    }

    #[test]
    fn d_returns_immediate_delete_action() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('d')));
        assert_eq!(action, Action::Delete(1));
    }

    #[test]
    fn u_returns_undo_and_r_returns_redo() {
        let mut app = make_app();
        assert_eq!(handle(&mut app, key(KeyCode::Char('u'))), Action::Undo);
        assert_eq!(handle(&mut app, key(KeyCode::Char('r'))), Action::Redo);
    }

    #[test]
    fn e_key_returns_edit_action() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('e')));
        assert_eq!(action, Action::EditTask(1));
    }

    #[test]
    fn o_key_returns_open_link_action() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('o')));
        assert_eq!(action, Action::OpenLink(1));
    }

    #[test]
    fn o_in_search_mode_types_instead_of_opening() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Char('/')));
        let action = handle(&mut app, key(KeyCode::Char('o')));
        assert_eq!(action, Action::Continue);
        assert_eq!(app.search_input.as_deref(), Some("o"));
    }

    #[test]
    fn enter_with_task_at_cursor_returns_edit_action() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Enter));
        assert_eq!(action, Action::EditTask(1));
    }

    #[test]
    fn enter_on_empty_list_is_noop() {
        let mut app = App::new(vec![]);
        let action = handle(&mut app, key(KeyCode::Enter));
        assert_eq!(action, Action::Continue);
    }

    #[test]
    fn digit_key_signals_reorder() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('3')));
        assert_eq!(action, Action::ReorderCursor(3));
    }

    #[test]
    fn digit_zero_does_nothing() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('0')));
        assert_eq!(action, Action::Continue);
    }

    #[test]
    fn shift_a_returns_set_category_a() {
        let mut app = make_app();
        let action = handle(&mut app, shift_key(KeyCode::Char('A')));
        assert_eq!(action, Action::SetCategory(1, Category::A));
    }

    #[test]
    fn esc_signals_quit_when_no_filter() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Esc));
        assert_eq!(action, Action::Quit);
    }

    #[test]
    fn esc_clears_filter_first_then_quits_on_second_press() {
        let mut app = make_app_with_text(&["Buy milk", "Read book"]);
        app.search_filter = "milk".into();
        let a1 = handle(&mut app, key(KeyCode::Esc));
        assert_eq!(a1, Action::Continue);
        assert!(app.search_filter.is_empty());
        assert_eq!(app.filtered_tasks().len(), 2);
        let a2 = handle(&mut app, key(KeyCode::Esc));
        assert_eq!(a2, Action::Quit);
    }

    #[test]
    fn q_does_not_quit() {
        let mut app = make_app();
        let action = handle(&mut app, key(KeyCode::Char('q')));
        assert_eq!(action, Action::Continue);
    }

    #[test]
    fn ctrl_c_signals_quit() {
        let mut app = make_app();
        let action = handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, Action::Quit);
    }

    // ── Search mode ────────────────────────────────────────────────────────────

    #[test]
    fn slash_enters_search_input_mode() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Char('/')));
        assert!(app.search_input.is_some());
        assert_eq!(app.search_input.as_deref(), Some(""));
    }

    #[test]
    fn typing_in_search_mode_filters_the_visible_list() {
        let mut app = make_app_with_text(&["Buy milk", "Read book", "Write blog"]);
        handle(&mut app, key(KeyCode::Char('/')));
        for c in "read".chars() {
            handle(&mut app, key(KeyCode::Char(c)));
        }
        let visible = app.filtered_tasks();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].text, "Read book");
    }

    #[test]
    fn enter_in_search_mode_commits_filter_and_exits_input() {
        let mut app = make_app_with_text(&["Buy milk", "Read book"]);
        handle(&mut app, key(KeyCode::Char('/')));
        for c in "milk".chars() {
            handle(&mut app, key(KeyCode::Char(c)));
        }
        handle(&mut app, key(KeyCode::Enter));
        assert!(app.search_input.is_none());
        assert_eq!(app.search_filter, "milk");
        assert_eq!(app.filtered_tasks().len(), 1);
    }

    #[test]
    fn esc_in_search_mode_cancels_input_and_keeps_prior_filter() {
        let mut app = make_app_with_text(&["Buy milk", "Read book"]);
        handle(&mut app, key(KeyCode::Char('/')));
        for c in "milk".chars() {
            handle(&mut app, key(KeyCode::Char(c)));
        }
        handle(&mut app, key(KeyCode::Enter));
        assert_eq!(app.search_filter, "milk");
        handle(&mut app, key(KeyCode::Char('/')));
        handle(&mut app, key(KeyCode::Backspace));
        handle(&mut app, key(KeyCode::Esc));
        assert!(app.search_input.is_none());
        assert_eq!(app.search_filter, "milk");
    }

    #[test]
    fn slash_pre_fills_with_current_filter() {
        let mut app = make_app_with_text(&["Buy milk"]);
        app.search_filter = "milk".into();
        handle(&mut app, key(KeyCode::Char('/')));
        assert_eq!(app.search_input.as_deref(), Some("milk"));
    }

    #[test]
    fn cursor_clamps_when_filter_narrows_the_list() {
        let mut app = make_app_with_text(&["Buy milk", "Read book", "Write blog"]);
        app.cursor = 2;
        handle(&mut app, key(KeyCode::Char('/')));
        for c in "milk".chars() {
            handle(&mut app, key(KeyCode::Char(c)));
        }
        assert_eq!(app.cursor, 0);
        assert_eq!(app.filtered_tasks().len(), 1);
    }

    #[test]
    fn text_input_in_search_mode_does_not_trigger_actions() {
        let mut app = make_app();
        handle(&mut app, key(KeyCode::Char('/')));
        let action = handle(&mut app, key(KeyCode::Char('d')));
        assert_eq!(action, Action::Continue);
        assert_eq!(app.search_input.as_deref(), Some("d"));
    }
}
