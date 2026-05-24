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
fn info_shows_task_details() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Buy milk", "p:a"])
        .assert()
        .success();
    task(&scope)
        .args(["info", "1"])
        .assert()
        .success()
        .stdout(contains("Buy milk"))
        .stdout(contains("Task #1"))
        .stdout(contains("A"));
}

#[test]
fn info_alias_show_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Task"]).assert().success();
    task(&scope)
        .args(["show", "1"])
        .assert()
        .success()
        .stdout(contains("Task #1"));
}

#[test]
fn info_nonexistent_task_fails() {
    let scope = StoreScope::new();
    task(&scope).args(["info", "99"]).assert().failure();
}

#[test]
fn info_format_md_outputs_markdown() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Buy milk", "p:a"])
        .assert()
        .success();
    task(&scope)
        .args(["--format", "md", "info", "1"])
        .assert()
        .success()
        .stdout(contains("# Task #1"))
        .stdout(contains("- **Text:** Buy milk"))
        .stdout(contains("- **Priority:** A"));
}
