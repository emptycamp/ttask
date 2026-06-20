mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use support::StoreScope;

fn task(scope: &StoreScope) -> Command {
    let mut cmd = Command::cargo_bin("ttask").unwrap();
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
fn list_human_view_is_ultra_mini_no_header_no_ord() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Active task", "c:a"])
        .assert()
        .success();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        // Compact single-line row, no table header / Ord column / Due.
        .stdout(contains("1 A Active task · 30m"))
        .stdout(predicates::str::contains("Description").not())
        .stdout(predicates::str::contains("Ord").not())
        .stdout(predicates::str::contains("Due").not());
}

#[test]
fn list_has_no_ab_estimate_footer() {
    // The A+B estimate / finish-time summary is TUI-only; `ttask ls` must not show it.
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Plan", "c:a", "est:1h"])
        .assert()
        .success();
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("A+B").not())
        .stdout(predicates::str::contains("finish").not());
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
    // Many tasks; ensure no "+N" overflow indicator appears. Use category C so the
    // A+B finish-time footer (which can legitimately carry a `+` for a next-day
    // finish) doesn't enter the picture.
    for i in 0..10 {
        task(&scope)
            .args(["add", &format!("task{i}"), "c:c"])
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

#[test]
fn list_with_id_shows_task_details() {
    // `ttask ls <id>` is a shortcut for `ttask show <id>` / `ttask info <id>`.
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "Buy milk", "c:a"])
        .assert()
        .success();
    task(&scope)
        .args(["ls", "1"])
        .assert()
        .success()
        .stdout(contains("Task #1"))
        .stdout(contains("Buy milk"))
        .stdout(contains("Category"));
}

#[test]
fn list_with_id_format_md_shows_task_card() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Read book"]).assert().success();
    task(&scope)
        .args(["--format", "md", "list", "1"])
        .assert()
        .success()
        .stdout(contains("# Task #1"))
        .stdout(contains("- **Text:** Read book"));
}

#[test]
fn list_shows_first_line_only_but_show_keeps_full_text() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "placeholder"]).assert().success();
    // Give the task a genuine multi-line description via the YAML edit hook (the
    // double-quoted scalar turns the literal `\n` into a real newline).
    let yaml = "text: \"line one\\nline two\"\ncategory: B\nord: 1\nest: 30m\n";
    task(&scope)
        .args(["edit", "1"])
        .env("TASK_EDIT_YAML", yaml)
        .assert()
        .success();

    // `ttask ls` shows only the first line with an ellipsis — multi-line text is not
    // flattened into one run-on row.
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("line one…"))
        .stdout(predicates::str::contains("line two").not());

    // `ttask show` keeps the line break in the details view.
    let out = task(&scope).args(["show", "1"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("line one"),
        "show missing first line:\n{stdout}"
    );
    assert!(
        stdout.contains("line two"),
        "show missing second line:\n{stdout}"
    );
    let one = stdout.find("line one").unwrap();
    let two = stdout.find("line two").unwrap();
    assert!(
        stdout[one..two].contains('\n'),
        "details view should keep the newline between the lines:\n{stdout}"
    );
}
