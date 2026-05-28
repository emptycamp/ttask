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
fn list_empty_store_shows_no_tasks() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("No tasks"));
}

#[test]
fn list_alias_ls_works() {
    let scope = StoreScope::new();
    task(&scope).args(["ls"]).assert().success();
}

#[test]
fn list_shows_active_tasks() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Active task"]).assert().success();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("Active task"));
}

#[test]
fn list_does_not_show_completed_tasks() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Done task"]).assert().success();
    task(&scope).args(["complete", "1"]).assert().success();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Done task").not());
}

#[test]
fn list_active_flag_shows_only_active() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Active"]).assert().success();
    task(&scope).args(["add", "Done"]).assert().success();
    task(&scope).args(["complete", "2"]).assert().success();
    task(&scope)
        .args(["list", "-a"])
        .assert()
        .success()
        .stdout(contains("Active"))
        .stdout(predicates::str::contains("Done").not());
}

#[test]
fn list_completed_flag_shows_only_completed() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Active"]).assert().success();
    task(&scope).args(["add", "Done task"]).assert().success();
    task(&scope).args(["complete", "2"]).assert().success();
    task(&scope)
        .args(["list", "--completed"])
        .assert()
        .success()
        .stdout(contains("Done task"))
        .stdout(predicates::str::contains("Active").not());
}

#[test]
fn list_completed_alias_done_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Done task"]).assert().success();
    task(&scope).args(["complete", "1"]).assert().success();
    task(&scope)
        .args(["list", "--done"])
        .assert()
        .success()
        .stdout(contains("Done task"));
}

#[test]
fn list_completed_alias_finished_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Done task"]).assert().success();
    task(&scope).args(["complete", "1"]).assert().success();
    task(&scope)
        .args(["list", "--finished"])
        .assert()
        .success()
        .stdout(contains("Done task"));
}

#[test]
fn list_deleted_flag_shows_only_deleted() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Keep me"]).assert().success();
    task(&scope).args(["add", "Trashed"]).assert().success();
    task(&scope).args(["delete", "2"]).assert().success();
    task(&scope)
        .args(["list", "--deleted"])
        .assert()
        .success()
        .stdout(contains("Trashed"))
        .stdout(predicates::str::contains("Keep me").not());
}

#[test]
fn list_deleted_alias_trash_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Trashed"]).assert().success();
    task(&scope).args(["delete", "1"]).assert().success();
    task(&scope)
        .args(["list", "--trash"])
        .assert()
        .success()
        .stdout(contains("Trashed"));
}

#[test]
fn list_all_shows_active_completed_and_deleted() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Active task"]).assert().success();
    task(&scope).args(["add", "Done task"]).assert().success();
    task(&scope)
        .args(["add", "Trashed task"])
        .assert()
        .success();
    task(&scope).args(["complete", "2"]).assert().success();
    task(&scope).args(["delete", "3"]).assert().success();
    task(&scope)
        .args(["list", "--all"])
        .assert()
        .success()
        .stdout(contains("Active task"))
        .stdout(contains("Done task"))
        .stdout(contains("Trashed task"));
}

#[test]
fn list_help_includes_examples_for_all_filters() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["list", "--help"])
        .assert()
        .success()
        .stdout(contains("--completed"))
        .stdout(contains("--deleted"))
        .stdout(contains("--active"))
        .stdout(contains("--all"))
        .stdout(contains("Examples"));
}

#[test]
fn list_output_includes_ord_column_header_not_due() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Active task"]).assert().success();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("ID"))
        .stdout(contains("Description"))
        .stdout(contains("Ord"))
        .stdout(contains("Est"))
        .stdout(predicates::str::contains("Due").not());
}

#[test]
fn list_does_not_show_day_headings() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "today task"]).assert().success();
    let out = task(&scope).args(["list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Today") && !stdout.contains("Tomorrow") && !stdout.contains("Yesterday"),
        "list view must be flat (no day groupings):\n{stdout}"
    );
}

#[test]
fn list_does_not_show_hidden_indicator() {
    let scope = StoreScope::new();
    // Many tasks; ensure no "+N" indicator appears.
    for i in 0..10 {
        task(&scope)
            .args(["add", &format!("task{i}")])
            .assert()
            .success();
    }
    let out = task(&scope).args(["list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('+'),
        "list must not show a +N hidden indicator:\n{stdout}"
    );
}

#[test]
fn list_orders_by_ord() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "first"]).assert().success();
    task(&scope).args(["add", "second"]).assert().success();
    // Move second to ord:1.
    task(&scope).args(["edit", "2", "ord:1"]).assert().success();
    let out = task(&scope).args(["list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let pos_second = stdout.find("second").unwrap();
    let pos_first = stdout.find("first").unwrap();
    assert!(
        pos_second < pos_first,
        "task with ord:1 should be listed first:\n{stdout}"
    );
}

#[test]
fn list_format_md_outputs_markdown_table_with_ord_not_due() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Buy milk"]).assert().success();
    task(&scope)
        .args(["--format", "md", "list"])
        .assert()
        .success()
        .stdout(contains("# Tasks"))
        .stdout(contains("| ID | Cat | Status | Ord | Description | Est |"))
        .stdout(contains("Buy milk"));
}

#[test]
fn list_format_md_empty_uses_no_tasks_italic() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["--format", "md", "list"])
        .assert()
        .success()
        .stdout(contains("_No tasks._"));
}
