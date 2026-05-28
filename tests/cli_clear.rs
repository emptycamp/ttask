mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use support::StoreScope;

fn task(scope: &StoreScope) -> Command {
    let mut cmd = Command::cargo_bin("task").unwrap();
    cmd.env("TASK_DATA_DIR", &scope.path);
    cmd
}

#[test]
fn clear_with_yes_wipes_tasks_and_history() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "first"]).assert().success();
    task(&scope).args(["add", "second"]).assert().success();

    task(&scope)
        .args(["clear", "-y"])
        .assert()
        .success()
        .stdout(contains("Cleared 2 tasks"))
        .stdout(contains("2 history events"));

    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("No tasks"));
    task(&scope)
        .args(["history", "list"])
        .assert()
        .success()
        .stdout(contains("No history"));
}

#[test]
fn clear_alias_wipe_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "x"]).assert().success();
    task(&scope).args(["wipe", "-y"]).assert().success();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("No tasks"));
}

#[test]
fn clear_alias_nuke_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "x"]).assert().success();
    task(&scope).args(["nuke", "-y"]).assert().success();
}

#[test]
fn clear_force_alias_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "x"]).assert().success();
    task(&scope).args(["clear", "--force"]).assert().success();
    task(&scope).args(["add", "y"]).assert().success();
    task(&scope).args(["clear", "-f"]).assert().success();
}

#[test]
fn clear_empty_store_succeeds() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["clear", "-y"])
        .assert()
        .success()
        .stdout(contains("Cleared 0 tasks"))
        .stdout(contains("0 history events"));
}

#[test]
fn clear_help_shows_warning_phrasing() {
    let scope = StoreScope::new();
    // The exact wording may wrap across lines depending on terminal width, so we
    // just check the key concepts are present.
    task(&scope)
        .args(["clear", "--help"])
        .assert()
        .success()
        .stdout(contains("Wipe"))
        .stdout(contains("undone"));
}

#[test]
fn next_id_resets_after_clear() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "first"]).assert().success();
    task(&scope).args(["add", "second"]).assert().success();
    task(&scope).args(["clear", "-y"]).assert().success();
    // After a wipe the next add should start fresh at #1.
    task(&scope)
        .args(["add", "fresh"])
        .assert()
        .success()
        .stdout(contains("Added task #1"));
    // And ensure the existing task list contains the new one only.
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("fresh"))
        .stdout(predicates::str::contains("first").not())
        .stdout(predicates::str::contains("second").not());
}
