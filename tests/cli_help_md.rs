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
fn help_with_format_md_emits_markdown_for_root() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["--help", "--format", "md"])
        .assert()
        .success()
        .stdout(contains("# ttask"))
        .stdout(contains("## Commands"))
        .stdout(contains("--format md"));
}

#[test]
fn help_with_format_md_works_in_either_order() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["--format", "md", "--help"])
        .assert()
        .success()
        .stdout(contains("# ttask"));
}

#[test]
fn help_with_short_h_flag_and_format_md_emits_markdown() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["-h", "--format", "md"])
        .assert()
        .success()
        .stdout(contains("# ttask"));
}

#[test]
fn help_with_format_md_for_subcommand_routes_correctly() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["list", "--help", "--format", "md"])
        .assert()
        .success()
        .stdout(contains("# ttask list"))
        .stdout(contains("--completed"));
}

#[test]
fn help_with_format_md_for_history_list_subcommand_routes_correctly() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["history", "list", "--help", "--format", "md"])
        .assert()
        .success()
        .stdout(contains("# ttask history list"));
}

#[test]
fn help_with_format_md_for_open_subcommand_routes_correctly() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["open", "--help", "--format", "md"])
        .assert()
        .success()
        .stdout(contains("# ttask open"))
        .stdout(contains("picker"));
}

#[test]
fn help_with_format_md_for_alias_normalizes_to_canonical_command() {
    // `ls` is the alias of `list`; markdown help should render the `list` page.
    let scope = StoreScope::new();
    task(&scope)
        .args(["ls", "--help", "--format", "md"])
        .assert()
        .success()
        .stdout(contains("# ttask list"));
}

#[test]
fn plain_help_without_format_md_is_unchanged() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["--help"])
        .assert()
        .success()
        .stdout(contains("Personal task manager"))
        .stdout(contains("Usage:"));
}

#[test]
fn format_md_alone_does_not_trigger_help() {
    // `ttask --format md` with no subcommand opens the TUI, which would error in a
    // non-tty test context. We just make sure the help text is NOT printed — the
    // markdown help should only show up when combined with `--help`.
    let scope = StoreScope::new();
    let out = task(&scope)
        .args(["--format", "md", "list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("## Commands"),
        "list output must not include the markdown root help: {stdout}"
    );
}
