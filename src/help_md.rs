//! Markdown-rendered help, used when the user combines `--help` and `--format md`.
//!
//! clap's built-in `--help` short-circuits parsing and writes its own plain output, so
//! this module hands back fully-formed markdown for the same surface. The match on
//! `path` mirrors the subcommand tree in `cli.rs`; if you add or rename a subcommand
//! there, update the corresponding branch (and its `path.matches` aliases) here too.

/// Returns true if `--format md` and `--help`/`-h` both appear in the raw args. We
/// scan once so order doesn't matter (`task --help --format md` and
/// `task --format md --help` both qualify).
pub fn wants_md_help(args: &[String]) -> bool {
    let has_help = args.iter().any(|a| a == "--help" || a == "-h");
    let has_md = args.windows(2).any(|w| w[0] == "--format" && w[1] == "md")
        || args.iter().any(|a| a == "--format=md");
    has_help && has_md
}

/// Extract the subcommand "path" from raw args, skipping `--format VALUE` and other
/// flags/values. We don't need to fully simulate clap; we just want to know which help
/// page the user is asking for. Returns e.g. `["history", "list"]` or `[]` for root.
pub fn extract_subcommand_path(args: &[String]) -> Vec<String> {
    let mut path = Vec::new();
    let mut i = 1; // skip program name
    while i < args.len() {
        let a = &args[i];
        if a == "--format" {
            // `--format md` — skip the value as well.
            i += 2;
            continue;
        }
        if a.starts_with("--format=") {
            i += 1;
            continue;
        }
        if a == "--help" || a == "-h" || a == "--version" || a == "-V" {
            i += 1;
            continue;
        }
        if a == "--test" {
            i += 1;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        // First positional arg is the subcommand. Keep walking to also pick up nested
        // subcommands like `history list`, but stop if we hit something that looks
        // like a positional payload (digits, free text).
        if let Some(canonical) = canonicalize(&path, a) {
            path.push(canonical);
            i += 1;
        } else {
            break;
        }
    }
    path
}

/// Map an alias to its canonical name for the given parent path. Returns `None` if
/// the token doesn't match any known (sub)command at this level — that's the cue to
/// stop walking and treat the remainder as positional args.
fn canonicalize(parent: &[String], token: &str) -> Option<String> {
    match parent
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => match token {
            "add" | "create" | "new" => Some("add".into()),
            "list" | "ls" => Some("list".into()),
            "edit" | "update" | "modify" | "change" | "set" => Some("edit".into()),
            "delete" | "del" | "remove" | "rm" | "discard" | "trash" => Some("delete".into()),
            "complete" | "done" | "finish" | "finished" | "close" => Some("complete".into()),
            "info" | "show" | "view" | "details" => Some("info".into()),
            "clear" | "wipe" | "nuke" | "reset" => Some("clear".into()),
            "history" | "log" | "events" => Some("history".into()),
            _ => None,
        },
        ["history"] => match token {
            "list" | "ls" => Some("list".into()),
            _ => None,
        },
        _ => None,
    }
}

/// Render the markdown help string for the given subcommand path.
pub fn render(path: &[String]) -> String {
    let path_strs: Vec<&str> = path.iter().map(String::as_str).collect();
    match path_strs.as_slice() {
        [] => ROOT.to_string(),
        ["add"] => ADD.to_string(),
        ["list"] => LIST.to_string(),
        ["edit"] => EDIT.to_string(),
        ["delete"] => DELETE.to_string(),
        ["complete"] => COMPLETE.to_string(),
        ["info"] => INFO.to_string(),
        ["clear"] => CLEAR.to_string(),
        ["history"] => HISTORY.to_string(),
        ["history", "list"] => HISTORY_LIST.to_string(),
        _ => format!(
            "# task {}\n\n_No markdown help for this subcommand. Run `task {} --help` for the plain version._\n",
            path.join(" "),
            path.join(" "),
        ),
    }
}

const ROOT: &str = "\
# task

Personal task manager.

## Usage

`task [OPTIONS] [COMMAND]`

If no command is given, the interactive TUI opens.

## Commands

- **add** `ARGS...` — Add a new task.
- **list** — List tasks (active by default).
- **edit** `ID [ARGS]...` — Edit a task. With no args, opens the form editor.
- **delete** `ID` — Soft-delete a task (recoverable via `history`).
- **complete** `ID` — Mark a task as completed.
- **info** `ID` — Show task details.
- **clear** — Wipe the entire database (irreversible).
- **history** — Show recent changes, or revert a specific event.

## Global Options

- `--format md` — Markdown output, optimized for LLM agents.
- `-h`, `--help` — Print help (combine with `--format md` for this view).
- `-V`, `--version` — Print version.

## Task Fields

When using `add` or `edit`, fields can be set inline:

- `p:a` | `p:b` | `p:c` — priority (A is highest)
- `due:tomorrow` | `due:fri` | `due:jun15` | `due:30m` — due date/time
- `est:30m` | `est:1h` | `est:2d` — estimated effort

Run `task <COMMAND> --help --format md` for command-specific markdown help.
";

const ADD: &str = "\
# task add

Add a new task.

## Usage

`task add ARGS...`

## Examples

- `task add Buy milk`
- `task add Read book p:a due:tomorrow est:1h`
- `task add Plan sprint due:jun15 est:2h p:b`
- `task add \"Quick chore\" p:c`

## Fields

- `p:a` | `p:b` | `p:c` — priority
- `due:tomorrow` | `due:fri` | `due:jun15` | `due:30m` — when it's due
- `est:30m` | `est:1h` — estimated effort
";

const LIST: &str = "\
# task list

List tasks. By default shows only active tasks in a compact view (today + the next
day with tasks, max 3 rows per day).

## Usage

`task list [OPTIONS]`

## Options

- `-a`, `--active` — Show only active tasks (default, but explicit form disables the
  compact cap).
- `--completed` — Show only completed tasks.
- `--deleted` — Show only soft-deleted tasks.
- `--all` — Show every task regardless of status.

## Examples

- `task list` — default compact view
- `task list -a` — full active list (no per-day cap)
- `task list --completed` — completed tasks
- `task list --all` — everything

In markdown mode each day becomes an `##` heading with a table of the task rows.
";

const EDIT: &str = "\
# task edit

Edit an existing task.

## Usage

`task edit ID [ARGS]...`

With no field args, opens the built-in form editor inside the terminal. The form
editor requires a real TTY; in scripts or piped contexts, pass field args
(`p:`/`due:`/`est:`/text) directly.

## Examples

- `task edit 3` — open the form editor
- `task edit 3 p:a` — set priority via args
- `task edit 3 New text` — change text via args
- `task edit 3 due:tomorrow est:30m`

In markdown mode the edited task is re-rendered as a full info card after the change.
";

const DELETE: &str = "\
# task delete

Delete a task. The task is soft-deleted and can be restored from `task history`.

## Usage

`task delete ID`

## Example

- `task delete 3`
";

const COMPLETE: &str = "\
# task complete

Mark a task as completed.

## Usage

`task complete ID`

## Example

- `task complete 3`
";

const INFO: &str = "\
# task info

Show full task details (text, priority, due, est, status, timestamps).

## Usage

`task info ID`

## Example

- `task info 3`

In markdown mode the output is a heading plus a bulleted list of fields.
";

const CLEAR: &str = "\
# task clear

Wipe the entire database — every task and every history event. This cannot be
undone. By default you get a confirmation prompt; pass `-y` / `-f` to skip it.

## Usage

`task clear [OPTIONS]`

## Options

- `-y`, `--yes` (`-f` / `--force`) — Skip the confirmation prompt.

## Examples

- `task clear` — confirm, then wipe
- `task clear -y` — no prompt
- `task clear --force` — equivalent to `-y`
";

const HISTORY: &str = "\
# task history

Show recent change history, or revert a specific event by ID.

## Usage

`task history [SUBCOMMAND] [OPTIONS]`

Running `task history` with no subcommand opens an interactive picker so you can
choose which event to undo. Use `task history list` to dump events to stdout.

The last 30 events are kept. Each event has a stable ID you can revert.

## Subcommands

- **list** `[-v]` — Print history events to stdout (no interactive picker).

## Options

- `--revert ID` — Revert the change with this history ID.
- `-y`, `--yes` (`-f` / `--force`) — Skip the confirmation prompt for `--revert`.

## Examples

- `task history` — interactive picker
- `task history list` — plain stdout list (minimal)
- `task history list -v` — include old→new diffs for edits
- `task history --revert 12` — revert event #12 (with confirmation)
- `task history --revert 12 -y` — skip confirmation
";

const HISTORY_LIST: &str = "\
# task history list

Print history events to stdout. This is the non-interactive form of `task history`
— it never opens the picker, so it's safe in scripts, pipes, and `--format md`
contexts.

## Usage

`task history list [-v]`

## Options

- `-v`, `--verbose` — Include detailed old→new values for edits. Without this,
  edits only show the field-name tokens (`text`, `p`, `due`, `est`) that changed.

In markdown mode the events come back as a single table with `ID`, `When`, and
`Event` columns. Revert operations always include the full diff regardless of this
flag — the user is acting on the change, so the extra detail is worth it.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        std::iter::once("task")
            .chain(parts.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn wants_md_help_detects_both_long_forms() {
        assert!(wants_md_help(&args(&["--help", "--format", "md"])));
        assert!(wants_md_help(&args(&["--format", "md", "--help"])));
    }

    #[test]
    fn wants_md_help_detects_short_help() {
        assert!(wants_md_help(&args(&["-h", "--format", "md"])));
    }

    #[test]
    fn wants_md_help_detects_equal_form() {
        assert!(wants_md_help(&args(&["--help", "--format=md"])));
    }

    #[test]
    fn wants_md_help_false_without_help() {
        assert!(!wants_md_help(&args(&["list", "--format", "md"])));
    }

    #[test]
    fn wants_md_help_false_without_format() {
        assert!(!wants_md_help(&args(&["--help"])));
    }

    #[test]
    fn extract_root_when_only_flags() {
        let path = extract_subcommand_path(&args(&["--help", "--format", "md"]));
        assert!(path.is_empty());
    }

    #[test]
    fn extract_canonicalizes_top_level_alias() {
        let path = extract_subcommand_path(&args(&["ls", "--help", "--format", "md"]));
        assert_eq!(path, vec!["list".to_string()]);
    }

    #[test]
    fn extract_walks_nested_history_list() {
        let path = extract_subcommand_path(&args(&["history", "list", "--help", "--format", "md"]));
        assert_eq!(path, vec!["history".to_string(), "list".to_string()]);
    }

    #[test]
    fn extract_history_log_alias_normalizes() {
        let path = extract_subcommand_path(&args(&["log", "--help", "--format", "md"]));
        assert_eq!(path, vec!["history".to_string()]);
    }

    #[test]
    fn extract_stops_at_unknown_positional() {
        // `info 7 --help`: "7" isn't a subcommand, so the walk stops at `info`.
        let path = extract_subcommand_path(&args(&["info", "7", "--help", "--format", "md"]));
        assert_eq!(path, vec!["info".to_string()]);
    }

    #[test]
    fn render_root_includes_global_format_option() {
        let out = render(&[]);
        assert!(out.starts_with("# task"));
        assert!(out.contains("--format md"));
        assert!(out.contains("## Commands"));
    }

    #[test]
    fn render_add_includes_field_syntax() {
        let out = render(&["add".into()]);
        assert!(out.starts_with("# task add"));
        assert!(out.contains("p:a"));
        assert!(out.contains("due:"));
        assert!(out.contains("est:"));
    }

    #[test]
    fn render_history_list_describes_subcommand() {
        let out = render(&["history".into(), "list".into()]);
        assert!(out.starts_with("# task history list"));
        assert!(out.contains("table"));
    }

    #[test]
    fn render_unknown_path_returns_a_useful_pointer() {
        let out = render(&["mystery".into()]);
        assert!(out.contains("No markdown help"));
        assert!(out.contains("task mystery --help"));
    }
}
