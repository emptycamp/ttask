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
fn edit_category_via_args() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Read book"]).assert().success();
    task(&scope).args(["edit", "1", "p:a"]).assert().success();
    task(&scope)
        .args(["info", "1"])
        .assert()
        .success()
        .stdout(contains("A"));
}

#[test]
fn edit_text_via_args() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Old text"]).assert().success();
    task(&scope)
        .args(["edit", "1", "New text"])
        .assert()
        .success();
    task(&scope)
        .args(["info", "1"])
        .assert()
        .success()
        .stdout(contains("New text"));
}

#[test]
fn edit_alias_update_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Task"]).assert().success();
    task(&scope).args(["update", "1", "p:c"]).assert().success();
}

#[test]
fn create_alias_works() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["create", "Buy milk"])
        .assert()
        .success()
        .stdout(contains("Added task #1"));
}

#[test]
fn edit_nonexistent_task_fails() {
    let scope = StoreScope::new();
    task(&scope).args(["edit", "99", "p:a"]).assert().failure();
}

#[test]
fn edit_with_form_via_yaml_env_var_updates_task() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Buy milk"]).assert().success();

    let new_content = "text: Buy oat milk\ncategory: A\nord: 1\nest: 15m\n";

    task(&scope)
        .args(["edit", "1"])
        .env("TASK_EDIT_YAML", new_content)
        .assert()
        .success();

    task(&scope)
        .args(["info", "1"])
        .assert()
        .success()
        .stdout(contains("Buy oat milk"));
}

#[test]
fn edit_form_cancel_via_env_var_leaves_task_unchanged() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Original"]).assert().success();

    task(&scope)
        .args(["edit", "1"])
        .env("TASK_EDIT_CANCEL", "1")
        .assert()
        .success();

    task(&scope)
        .args(["info", "1"])
        .assert()
        .success()
        .stdout(contains("Original"));
}
