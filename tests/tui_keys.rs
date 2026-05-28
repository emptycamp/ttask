use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use task::model::{Category, Status, Task};
use task::tui::events::{handle, Action, PendingChange};
use task::tui::App;

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
    App::new(vec![make_task(1), make_task(2), make_task(3)])
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn navigate_down_and_up() {
    let mut app = make_app();
    handle(&mut app, press(KeyCode::Down));
    assert_eq!(app.cursor, 1);
    handle(&mut app, press(KeyCode::Up));
    assert_eq!(app.cursor, 0);
}

#[test]
fn toggle_complete_adds_and_removes() {
    let mut app = make_app();
    handle(&mut app, press(KeyCode::Char('c')));
    assert!(app.pending[&1].contains(&PendingChange::ToggleComplete(1)));
    handle(&mut app, press(KeyCode::Char('c')));
    assert!(app.pending.get(&1).map(|v| v.is_empty()).unwrap_or(true));
}

#[test]
fn toggle_delete_adds_and_removes() {
    let mut app = make_app();
    handle(&mut app, press(KeyCode::Char('d')));
    assert!(app.pending[&1].contains(&PendingChange::ToggleDelete(1)));
}

#[test]
fn set_category_replaces_existing() {
    let mut app = make_app();
    handle(
        &mut app,
        KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT),
    );
    handle(
        &mut app,
        KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
    );
    let changes = &app.pending[&1];
    assert!(!changes.contains(&PendingChange::SetCategory(1, Category::A)));
    assert!(changes.contains(&PendingChange::SetCategory(1, Category::C)));
}

#[test]
fn e_returns_edit_action() {
    let mut app = make_app();
    let action = handle(&mut app, press(KeyCode::Char('e')));
    assert_eq!(action, Action::EditTask(1));
}

#[test]
fn enter_with_task_at_cursor_returns_edit_action() {
    let mut app = make_app();
    let action = handle(&mut app, press(KeyCode::Enter));
    assert_eq!(action, Action::EditTask(1));
}

#[test]
fn digit_key_signals_reorder() {
    let mut app = make_app();
    let action = handle(&mut app, press(KeyCode::Char('2')));
    assert_eq!(action, Action::ReorderCursor(2));
}

#[test]
fn esc_exits() {
    let mut app = make_app();
    let action = handle(&mut app, press(KeyCode::Esc));
    assert_eq!(action, Action::Quit);
}
