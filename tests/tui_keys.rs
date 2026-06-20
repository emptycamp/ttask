use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ttask::model::{Category, Status, Task};
use ttask::tui::events::{handle, Action};
use ttask::tui::App;

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
fn c_signals_immediate_complete() {
    let mut app = make_app();
    let action = handle(&mut app, press(KeyCode::Char('c')));
    assert_eq!(action, Action::Complete(1));
}

#[test]
fn d_signals_immediate_delete() {
    let mut app = make_app();
    let action = handle(&mut app, press(KeyCode::Char('d')));
    assert_eq!(action, Action::Delete(1));
}

#[test]
fn shift_category_signals_set_category() {
    let mut app = make_app();
    let action = handle(
        &mut app,
        KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT),
    );
    assert_eq!(action, Action::SetCategory(1, Category::A));
}

#[test]
fn u_and_r_signal_undo_redo() {
    let mut app = make_app();
    assert_eq!(handle(&mut app, press(KeyCode::Char('u'))), Action::Undo);
    assert_eq!(handle(&mut app, press(KeyCode::Char('r'))), Action::Redo);
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
fn a_signals_add() {
    let mut app = make_app();
    let action = handle(&mut app, press(KeyCode::Char('a')));
    assert_eq!(action, Action::AddTask);
}

#[test]
fn esc_exits() {
    let mut app = make_app();
    let action = handle(&mut app, press(KeyCode::Esc));
    assert_eq!(action, Action::Quit);
}
