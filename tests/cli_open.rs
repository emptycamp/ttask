mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use support::StoreScope;

fn task(scope: &StoreScope) -> Command {
    let mut cmd = Command::cargo_bin("task").unwrap();
    cmd.env("TASK_DATA_DIR", &scope.path);
    // Never actually spawn a browser from the test suite.
    cmd.env("TASK_OPEN_DRY_RUN", "1");
    cmd
}

#[test]
fn open_single_link_reports_url() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "see https://example.com for info"])
        .assert()
        .success();
    task(&scope)
        .args(["open", "1"])
        .assert()
        .success()
        .stdout(contains("Opening"))
        .stdout(contains("https://example.com"));
}

#[test]
fn open_alias_o_works() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "docs https://docs.rs/clap"])
        .assert()
        .success();
    task(&scope)
        .args(["o", "1"])
        .assert()
        .success()
        .stdout(contains("https://docs.rs/clap"));
}

#[test]
fn open_task_with_no_links_fails() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "plain task with no links"])
        .assert()
        .success();
    task(&scope)
        .args(["open", "1"])
        .assert()
        .failure()
        .stderr(contains("no links"));
}

#[test]
fn open_with_index_picks_that_link() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "one https://a.example two https://b.example"])
        .assert()
        .success();
    task(&scope)
        .args(["open", "1", "2"])
        .assert()
        .success()
        .stdout(contains("https://b.example"))
        .stdout(contains("https://a.example").not());
}

#[test]
fn open_index_out_of_range_fails() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "only https://a.example here"])
        .assert()
        .success();
    task(&scope)
        .args(["open", "1", "5"])
        .assert()
        .failure()
        .stderr(contains("out of range"));
}

#[test]
fn open_multiple_links_without_index_lists_them_when_non_interactive() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "first https://a.example second https://b.example"])
        .assert()
        .success();
    // assert_cmd's stdout is not a TTY, so the picker is skipped and we get a
    // numbered list plus guidance to pass the link number.
    task(&scope)
        .args(["open", "1"])
        .assert()
        .failure()
        .stderr(contains("https://a.example"))
        .stderr(contains("https://b.example"))
        .stderr(contains("link number"));
}

#[test]
fn open_format_md_reports_opened_url() {
    let scope = StoreScope::new();
    task(&scope)
        .args(["add", "ref https://example.com/x"])
        .assert()
        .success();
    task(&scope)
        .args(["--format", "md", "open", "1"])
        .assert()
        .success()
        .stdout(contains("**Opening**"))
        .stdout(contains("https://example.com/x"));
}

#[test]
fn open_nonexistent_task_fails() {
    let scope = StoreScope::new();
    task(&scope).args(["open", "99"]).assert().failure();
}
