mod support;

use assert_cmd::Command;
use predicates::str::contains;
use support::StoreScope;

fn task(scope: &StoreScope) -> Command {
    let mut cmd = Command::cargo_bin("task").unwrap();
    cmd.env("TASK_DATA_DIR", &scope.path);
    cmd
}

#[test]
fn add_basic_task_prints_confirmation() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Buy milk"])
        .assert()
        .success()
        .stdout(contains("Added task #1"));
}

#[test]
fn add_task_with_category_a() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Read book", "p:a"])
        .assert()
        .success();
}

#[test]
fn add_task_with_ord_and_est() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Plan sprint", "ord:1", "est:1h"])
        .assert()
        .success();
}

#[test]
fn add_multiple_tasks_increments_ids() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Task one"]).assert().success();
    task(&scope)
        .args(["add", "Task two"])
        .assert()
        .success()
        .stdout(contains("#2"));
}

#[test]
fn add_no_text_returns_error() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "p:a"]).assert().failure();
}

#[test]
fn add_no_args_opens_editor_and_creates_task() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add"])
        .env(
            "TASK_EDIT_YAML",
            "text: Editor task\ncategory: A\nord: 1\nest: 15m\n",
        )
        .assert()
        .success()
        .stdout(contains("Added task #1"));
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("Editor task"));
}

#[test]
fn add_no_args_cancelled_editor_creates_nothing() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add"])
        .env("TASK_EDIT_CANCEL", "1")
        .assert()
        .success();
    // Nothing was created and no id was consumed, so the next real add is #1.
    task(&scope)
        .args(["add", "Real task"])
        .assert()
        .success()
        .stdout(contains("Added task #1"));
}

#[test]
fn add_appears_in_list() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Buy groceries"])
        .assert()
        .success();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("Buy groceries"));
}
