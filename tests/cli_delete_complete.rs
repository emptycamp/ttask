mod support;

use assert_cmd::Command;
use predicates::str::contains;
use support::StoreScope;

fn task(scope: &StoreScope) -> Command {
    let mut cmd = Command::cargo_bin("ttask").unwrap();
    cmd.env("TASK_DATA_DIR", &scope.path);
    cmd
}

#[test]
fn delete_removes_task_from_active_list() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "To delete"]).assert().success();
    task(&scope).args(["delete", "1"]).assert().success();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("To delete").not());
}

#[test]
fn delete_alias_del_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Task"]).assert().success();
    task(&scope).args(["del", "1"]).assert().success();
}

#[test]
fn delete_alias_rm_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Task"]).assert().success();
    task(&scope).args(["rm", "1"]).assert().success();
}

#[test]
fn delete_alias_remove_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Task"]).assert().success();
    task(&scope).args(["remove", "1"]).assert().success();
}

#[test]
fn delete_nonexistent_task_fails() {
    let scope = StoreScope::new();
    task(&scope).args(["delete", "99"]).assert().failure();
}

#[test]
fn complete_marks_task_as_done() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Finish report"])
        .assert()
        .success();
    task(&scope).args(["complete", "1"]).assert().success();
    task(&scope)
        .args(["list", "--done"])
        .assert()
        .success()
        .stdout(contains("Finish report"));
}

#[test]
fn complete_alias_done_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Task"]).assert().success();
    task(&scope).args(["done", "1"]).assert().success();
}

#[test]
fn complete_nonexistent_task_fails() {
    let scope = StoreScope::new();
    task(&scope).args(["complete", "99"]).assert().failure();
}

use predicates::prelude::*;
