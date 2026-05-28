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
fn tasks_persist_across_invocations() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Persistent task"])
        .assert()
        .success();
    // Second invocation — new process, same data dir
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("Persistent task"));
}

#[test]
fn completed_tasks_persist() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Complete me"]).assert().success();
    task(&scope).args(["complete", "1"]).assert().success();
    task(&scope)
        .args(["list", "--done"])
        .assert()
        .success()
        .stdout(contains("Complete me"));
}
