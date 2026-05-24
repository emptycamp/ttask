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

fn extract_event_id(output: &str) -> Option<u64> {
    output
        .lines()
        .filter(|line| {
            // Skip header rows
            !line.contains("ID") && !line.contains("─") && !line.trim().is_empty()
        })
        .find_map(|line| line.trim_start().split_whitespace().next()?.parse().ok())
}

#[test]
fn history_empty_when_no_events() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["history"])
        .assert()
        .success()
        .stdout(contains("No history"));
}

#[test]
fn history_lists_events_after_adds() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "First"]).assert().success();
    task(&scope).args(["add", "Second"]).assert().success();
    task(&scope)
        .args(["history"])
        .assert()
        .success()
        .stdout(contains("added #1"))
        .stdout(contains("added #2"));
}

#[test]
fn history_list_subcommand_is_explicit_list() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "First"]).assert().success();
    task(&scope)
        .args(["history", "list"])
        .assert()
        .success()
        .stdout(contains("added #1"));
}

#[test]
fn history_list_alias_ls_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "First"]).assert().success();
    task(&scope)
        .args(["history", "ls"])
        .assert()
        .success()
        .stdout(contains("added #1"));
}

#[test]
fn history_list_dashes_flag_is_no_longer_recognized() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "First"]).assert().success();
    task(&scope).args(["history", "--list"]).assert().failure();
}

#[test]
fn history_when_column_is_relative() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "First"]).assert().success();
    let output = task(&scope).args(["history", "list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain something like "just now" or "Xm ago" — never an absolute date.
    let has_relative = stdout.contains("just now") || stdout.contains("ago");
    assert!(has_relative, "expected relative When, got:\n{stdout}");
    // No YYYY-MM-DD format remains.
    let has_absolute =
        stdout.contains("2026-") || stdout.contains("2025-") || stdout.contains("2027-");
    assert!(
        !has_absolute,
        "expected no absolute timestamps, got:\n{stdout}"
    );
}

#[test]
fn history_revert_removes_added_task() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Temporary"]).assert().success();

    let output = task(&scope).args(["history"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let event_id = extract_event_id(&stdout).expect("expected an event id");

    task(&scope)
        .args(["history", "--revert", &event_id.to_string(), "-y"])
        .assert()
        .success();

    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("No tasks"));
}

#[test]
fn history_revert_restores_deleted_task() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Important"]).assert().success();
    task(&scope).args(["delete", "1"]).assert().success();

    let output = task(&scope).args(["history"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let event_id = stdout
        .lines()
        .find(|l| l.contains("deleted"))
        .and_then(|l| {
            l.trim_start()
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .expect("expected a deleted event");

    task(&scope)
        .args(["history", "--revert", &event_id.to_string(), "-y"])
        .assert()
        .success();

    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("Important"));
}

#[test]
fn history_revert_complete_restores_to_active() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Not done"]).assert().success();
    task(&scope).args(["complete", "1"]).assert().success();

    let output = task(&scope).args(["history"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let event_id = stdout
        .lines()
        .find(|l| l.contains("completed"))
        .and_then(|l| {
            l.trim_start()
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .expect("expected a completed event");

    task(&scope)
        .args(["history", "--revert", &event_id.to_string(), "-y"])
        .assert()
        .success();

    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("Not done"));
}

#[test]
fn history_revert_unknown_event_fails() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Anything"]).assert().success();
    task(&scope)
        .args(["history", "--revert", "9999", "-y"])
        .assert()
        .failure();
}

#[test]
fn history_caps_at_30_events() {
    let scope = StoreScope::new();
    // Add 35 tasks → 35 events
    for i in 0..35 {
        task(&scope)
            .args(["add", &format!("task {i}")])
            .assert()
            .success();
    }

    let output = task(&scope).args(["history"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let event_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.contains("ID") && !l.contains("─") && !l.trim().is_empty())
        .collect();
    assert_eq!(event_lines.len(), 30);
}

#[test]
fn history_revert_accepts_force_short_alias() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Temporary"]).assert().success();

    let output = task(&scope).args(["history"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let event_id = extract_event_id(&stdout).expect("expected an event id");

    task(&scope)
        .args(["history", "--revert", &event_id.to_string(), "-f"])
        .assert()
        .success();
}

#[test]
fn history_revert_accepts_force_long_alias() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Temporary"]).assert().success();

    let output = task(&scope).args(["history"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let event_id = extract_event_id(&stdout).expect("expected an event id");

    task(&scope)
        .args(["history", "--revert", &event_id.to_string(), "--force"])
        .assert()
        .success();
}

#[test]
fn history_revert_independent_tasks_do_not_cascade() {
    // Three unrelated adds. Reverting the oldest should only undo that one task —
    // separate tasks aren't connected.
    let scope = StoreScope::new();
    task(&scope).args(["add", "first"]).assert().success();
    task(&scope).args(["add", "second"]).assert().success();
    task(&scope).args(["add", "third"]).assert().success();

    let output = task(&scope).args(["history", "list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let oldest_id = stdout
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .min()
        .expect("expected at least one event");

    task(&scope)
        .args(["history", "--revert", &oldest_id.to_string(), "-y"])
        .assert()
        .success()
        .stdout(contains("Reverted event"));

    // Only the first task is gone; second and third survive.
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("second"))
        .stdout(contains("third"))
        .stdout(predicates::str::contains("first").not());
}

#[test]
fn history_revert_cascades_through_same_task_only() {
    // Task #1 gets added, edited, completed. Task #2 just gets added.
    // Reverting the add-of-task-#1 cascades through task #1's three events,
    // but leaves task #2 alone.
    let scope = StoreScope::new();
    task(&scope).args(["add", "first"]).assert().success(); // event #1: Added id=1
    task(&scope).args(["add", "second"]).assert().success(); // event #2: Added id=2
    task(&scope)
        .args(["edit", "1", "renamed"])
        .assert()
        .success(); // event #3: Edited id=1
    task(&scope).args(["complete", "1"]).assert().success(); // event #4: Completed id=1

    task(&scope)
        .args(["history", "--revert", "1", "-y"])
        .assert()
        .success()
        .stdout(contains("Reverted 3 events"));

    // Task #2 untouched.
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("second"));
    // Task #1 gone (the cascade removed all three of its events).
    task(&scope).args(["info", "1"]).assert().failure();
}

#[test]
fn history_revert_latest_only_reverts_one() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "first"]).assert().success();
    task(&scope).args(["add", "second"]).assert().success();

    let output = task(&scope).args(["history", "list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let latest_id = stdout
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .max()
        .expect("expected at least one event");

    task(&scope)
        .args(["history", "--revert", &latest_id.to_string(), "-y"])
        .assert()
        .success()
        .stdout(contains("Reverted event"));

    // The older task survives, the newer is gone.
    task(&scope)
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("first"))
        .stdout(predicates::str::contains("second").not());
}

#[test]
fn history_alias_log_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Task"]).assert().success();
    task(&scope)
        .args(["log"])
        .assert()
        .success()
        .stdout(contains("added #1"));
}

#[test]
fn history_format_md_outputs_markdown_table() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "First"]).assert().success();
    task(&scope)
        .args(["--format", "md", "history"])
        .assert()
        .success()
        .stdout(contains("# History"))
        .stdout(contains("| ID | When | Event |"))
        .stdout(contains("added #1"));
}

#[test]
fn list_format_md_with_add_outputs_info_card() {
    // `task add --format md` should render the full new task as a markdown info card.
    let scope = StoreScope::new();
    task(&scope)
        .args(["--format", "md", "add", "Hello"])
        .assert()
        .success()
        .stdout(contains("# Task #1"))
        .stdout(contains("- **Text:** Hello"));
}

#[test]
fn history_list_default_summary_is_minimal_for_edits() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Buy milk"]).assert().success();
    task(&scope)
        .args(["edit", "1", "Buy almond milk", "p:a"])
        .assert()
        .success();
    let output = task(&scope)
        .args(["history", "list"])
        .output()
        .expect("history list");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Minimal: only the field tokens, no quoted before/after values, no arrows.
    assert!(
        stdout.contains("edited #1: text, p"),
        "expected minimal field tokens, got:\n{stdout}"
    );
    assert!(
        !stdout.contains('→'),
        "default summary must not include old→new arrows:\n{stdout}"
    );
    assert!(
        !stdout.contains("Buy almond milk"),
        "default summary must not include the new text value:\n{stdout}"
    );
}

#[test]
fn history_list_verbose_includes_diff_for_edits() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Buy milk"]).assert().success();
    task(&scope)
        .args(["edit", "1", "Buy almond milk", "p:a"])
        .assert()
        .success();
    task(&scope)
        .args(["history", "list", "-v"])
        .assert()
        .success()
        .stdout(contains("text \"Buy milk\"→\"Buy almond milk\""))
        .stdout(contains("p B→A"));
}

#[test]
fn history_list_verbose_long_flag_works() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "Buy milk"]).assert().success();
    task(&scope)
        .args(["edit", "1", "Buy almond milk"])
        .assert()
        .success();
    task(&scope)
        .args(["history", "list", "--verbose"])
        .assert()
        .success()
        .stdout(contains("text \"Buy milk\"→\"Buy almond milk\""));
}

#[test]
fn history_list_non_edit_events_unchanged_by_verbose_flag() {
    // Added/Deleted/Completed already carry the text in minimal mode — verbose
    // shouldn't alter them.
    let scope = StoreScope::new();
    task(&scope).args(["add", "Quick chore"]).assert().success();
    let minimal = task(&scope).args(["history", "list"]).output().unwrap();
    let verbose = task(&scope)
        .args(["history", "list", "-v"])
        .output()
        .unwrap();
    let m = String::from_utf8_lossy(&minimal.stdout);
    let v = String::from_utf8_lossy(&verbose.stdout);
    assert!(m.contains("added #1: Quick chore"));
    assert!(v.contains("added #1: Quick chore"));
}

#[test]
fn history_list_format_md_always_renders_verbose_diff() {
    // Md mode is for LLM agents — they want the full diff on every event without
    // having to remember a flag. The `-v` flag still works but is a no-op in md.
    let scope = StoreScope::new();
    task(&scope).args(["add", "x"]).assert().success();
    task(&scope).args(["edit", "1", "y"]).assert().success();
    task(&scope)
        .args(["--format", "md", "history", "list"])
        .assert()
        .success()
        .stdout(contains("text \"x\"→\"y\""));
}

#[test]
fn history_list_format_md_with_verbose_flag_is_still_verbose() {
    let scope = StoreScope::new();
    task(&scope).args(["add", "x"]).assert().success();
    task(&scope).args(["edit", "1", "y"]).assert().success();
    task(&scope)
        .args(["--format", "md", "history", "list", "-v"])
        .assert()
        .success()
        .stdout(contains("text \"x\"→\"y\""));
}

#[test]
fn history_revert_message_uses_verbose_summary_even_in_default_mode() {
    // Revert confirmations and "Reverted event #N: ..." should always carry the full
    // diff, since the user is acting on the change and wants to see what it was.
    let scope = StoreScope::new();
    task(&scope).args(["add", "Buy milk"]).assert().success();
    task(&scope)
        .args(["edit", "1", "Buy almond milk"])
        .assert()
        .success();
    let output = task(&scope).args(["history", "list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let edit_event_id = stdout
        .lines()
        .find(|l| l.contains("edited #1"))
        .and_then(|l| {
            l.trim_start()
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .expect("expected an edit event");
    task(&scope)
        .args(["history", "--revert", &edit_event_id.to_string(), "-y"])
        .assert()
        .success()
        .stdout(contains("text \"Buy milk\"→\"Buy almond milk\""));
}
