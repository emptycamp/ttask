//! Markdown-rendered help, used when the user combines `--help` and `--format md`.

pub fn wants_md_help(args: &[String]) -> bool {
    let has_help = args.iter().any(|a| a == "--help" || a == "-h");
    let has_md = args.windows(2).any(|w| w[0] == "--format" && w[1] == "md")
        || args.iter().any(|a| a == "--format=md");
    has_help && has_md
}

pub fn extract_subcommand_path(args: &[String]) -> Vec<String> {
    let mut path = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "--format" {
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
        if let Some(canonical) = canonicalize(&path, a) {
            path.push(canonical);
            i += 1;
        } else {
            break;
        }
    }
    path
}

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
            "edit" | "e" | "update" | "modify" | "change" | "set" => Some("edit".into()),
            "delete" | "del" | "remove" | "rm" | "discard" | "trash" => Some("delete".into()),
            "complete" | "done" | "finish" | "finished" | "close" => Some("complete".into()),
            "info" | "show" | "view" | "details" => Some("info".into()),
            "open" | "o" | "launch" => Some("open".into()),
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
        ["open"] => OPEN.to_string(),
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

## ⚠ Notice for LLM agents

**Never invoke any of the interactive TUI surfaces.** They block on user input
forever and will hang your script / tool call. Specifically:

- `task` with **no command** — opens the main TUI. Don't.
- `task edit <ID>` with **no field args** — opens the form editor. Always pass
  fields (`task edit 3 c:a`, `task edit 3 ord:1`, etc.).
- `task history` with **no subcommand** — opens the history picker. Use
  `task history list` instead (and add `--format md` to get a table).
- `task clear` without `-y` — opens a confirmation prompt. Pass `-y` to skip.
- `task open <ID>` when the task has **several links** — opens a picker. Pass the
  link number (`task open 3 2`) to avoid it.

For machine-readable output, append `--format md` to any non-interactive
subcommand (`task list --format md`, `task info 3 --format md`,
`task history list --format md`, etc.).

## Usage

`task [OPTIONS] [COMMAND]`

If no command is given, the interactive TUI opens.

## Commands

- **add** `[ARGS]...` — Add a new task; with no args, opens the editor (TTY only).
- **list** `[ID]` — List tasks (active by default); with an ID, show that task's details.
- **edit** `ID [ARGS]...` — Edit a task. With no args, opens the text editor.
- **delete** `ID` — Soft-delete a task (recoverable via `history`).
- **complete** `ID` — Mark a task as completed.
- **info** `ID` — Show task details.
- **open** `ID [N]` — Open a link from a task's text (picker if several; or pass N).
- **clear** — Wipe the entire database (irreversible).
- **history** — Show recent changes, or revert a specific event.

## TUI keys

Run `task` with no command to open the interactive list. Then:

- `↑` / `↓` — move cursor between tasks
- `1`..`9` — move the cursor task to that 1-based position within its category
- `Enter` / `e` — edit the task at the cursor (text + estimate)
- `/` — search/filter the task list; type to filter, `Enter` to apply, `Esc` to
  cancel the edit
- `a` — add a task
- `o` — open a link from the cursor task's text (picker if it has several)
- `c` / `d` — complete / delete the cursor task (applied immediately)
- `Shift+A` / `Shift+B` / `Shift+C` — set category on the cursor task (immediate)
- `u` / `r` — undo / redo the last change made this session. Undo/redo history is
  cleared when you quit; after that, roll back via `task history`.
- `Esc` — clear an active search filter on the first press; quit on the next.
  `Ctrl+C` quits unconditionally.

## Global Options

- `--format md` — Markdown output, optimized for LLM agents.
- `-h`, `--help` — Print help (combine with `--format md` for this view).
- `-V`, `--version` — Print version.

## Task Fields

When using `add` or `edit`, fields can be set inline:

- `c:a` | `c:b` | `c:c` — category (A is highest)
- `ord:N` — manual order position (1-based, within the category)
- `est:30m` | `est:1h` | `est:2d` — estimated effort
- A bare duration token at the start or end of the text (e.g. `Buy milk 30m`)
  also sets the estimate. An explicit `est:` wins if both are present.

Order is tracked per-category: A, B, and C each have their own 1-based sequence.

## Auto-deletion

Stale tasks are removed automatically. Ages are measured in **working days**
(Mon–Fri local time; weekends are skipped). Any user edit to a task — text,
category, ord, or est — resets its clock.

- **Category A** — never auto-deleted.
- **Category B** — auto-deleted after **1 work week** (5 working days) without
  any update.
- **Category C** — auto-deleted after **2 working days** without any update.
- **Completed** — hard-removed **1 work week** after completion.
- **Soft-deleted** — hard-removed **1 work week** after deletion. (Until then
  the task is recoverable via `task history`.)

Run `task <COMMAND> --help --format md` for command-specific markdown help.
";

const ADD: &str = "\
# task add

Add a new task. With no arguments, opens the built-in text editor in the terminal
(the same form as the `a` key in the `task` view); type the text and `Esc` to save.

## Usage

`task add [ARGS]...`

## ⚠ Notice for LLM agents

**Always pass the task text as args.** Calling `task add` with no arguments opens
the interactive editor and will hang your tool call (it needs a TTY). Pass the text
directly instead — `task add Buy milk`, optionally with `c:` / `ord:` / `est:`
fields and a trailing/leading duration.

## Examples

- `task add Buy milk`
- `task add Read book c:a est:1h`
- `task add Plan sprint c:b est:2h ord:1`
- `task add \"Quick chore\" c:c`

## Fields

- `c:a` | `c:b` | `c:c` — category
- `ord:N` — manual order position (1-based); other tasks shift to make room
- `est:30m` | `est:1h` — estimated effort

## Auto-deletion by category

Stale tasks are swept automatically based on how long it's been since the last
edit. Working days only (Mon–Fri).

- **A** — never auto-deleted.
- **B** — gone after **1 work week** (5 working days) of no updates.
- **C** — gone after **2 working days** of no updates.

Editing the task (text, category, ord, est) resets the clock.
";

const LIST: &str = "\
# task list

List tasks. By default shows only active tasks, grouped by category (A, then B,
then C) and ordered within each category by its manual order.

Given a task ID, shows that single task's full details instead — `task list 3` is a
shortcut for `task info 3`.

## Usage

`task list [ID] [OPTIONS]`

## Options

- `-a`, `--active` — Show only active tasks (default).
- `--completed` — Show only completed tasks.
- `--deleted` — Show only soft-deleted tasks.
- `--all` — Show every task regardless of status.

## Examples

- `task list` — active tasks, grouped by category
- `task list 3` — full details for task #3 (like `task info 3`)
- `task list --completed` — completed tasks
- `task list --all` — everything

The human view is ultra-compact (`1 A Buy milk · 30m`, no Ord column). The
combined A+B estimate and projected finish time are shown only in the interactive
`task` view, not in `task list`.

In markdown mode tasks come back as a single table with `ID`, `Cat`, `Status`,
`Ord`, `Description`, and `Est` columns.
";

const EDIT: &str = "\
# task edit

Edit an existing task.

## Usage

`task edit ID [ARGS]...`

With no field args, opens the built-in text editor inside the terminal — an
interactive TUI that blocks on input. `Enter` inserts a newline (tasks may carry a
multi-line description); `Esc` saves and exits. A duration token at the end of
the text (e.g. `Buy milk 45m`) sets the
estimate, including on a multi-line description; on a single-line task a leading
token works too. Category and ord are not editable from the editor.

## ⚠ Notice for LLM agents

**Always pass at least one field arg.** Calling `task edit 3` with no fields
opens the interactive editor and will hang your tool call. Use the inline field
syntax instead — `task edit 3 c:a`, `task edit 3 ord:1 est:30m`, or
`task edit 3 New text here` (anything not prefixed with `c:` / `ord:` /
`est:` is treated as the new task text, and a trailing/leading duration sets the
estimate).

## Examples

- `task edit 3 c:a` — set category via args
- `task edit 3 New text` — change text via args
- `task edit 3 New text 45m` — change text and set estimate (bare token)
- `task edit 3 ord:1 est:30m` — move to first position and update estimate

In markdown mode the edited task is re-rendered as a full info card after the change.
";

const DELETE: &str = "\
# task delete

Delete a task. The task is soft-deleted and can be restored from `task history`.

## Usage

`task delete ID`

## Example

- `task delete 3`

## Retention

After soft-delete, the task is hard-removed automatically **1 work week**
(5 working days) later. Until then it can be restored via `task history`.
";

const COMPLETE: &str = "\
# task complete

Mark a task as completed.

## Usage

`task complete ID`

## Example

- `task complete 3`

## Retention

Completed tasks are hard-removed automatically **1 work week** (5 working
days) after completion.
";

const INFO: &str = "\
# task info

Show full task details (text, category, ord, est, status, timestamps).

## Usage

`task info ID`

## Example

- `task info 3`

In markdown mode the output is a heading plus a bulleted list of fields.
";

const OPEN: &str = "\
# task open

Open a link contained in a task's text using the system's default handler
(the default browser on Windows, `open` on macOS, `xdg-open` on Linux).

## Usage

`task open ID [INDEX]`

The task text is scanned for URLs (`http://`, `https://`, or a leading `www.`):

- no links — an error.
- exactly one link — it is opened.
- several links — opens an interactive picker, **unless** you pass the 1-based
  `INDEX` of the link to open.

## ⚠ Notice for LLM agents

With several links and no `INDEX`, `task open` opens an interactive picker that
blocks. **Always pass the link number** (`task open 3 2`). In a non-interactive
context the command instead errors and lists the numbered links, so you can re-run
with the right index. `task open <ID> 1` opens the first link non-interactively.

## Examples

- `task open 3` — open the only link in task #3 (or pick one if there are several)
- `task open 3 2` — open the 2nd link in task #3 (no picker)
";

const CLEAR: &str = "\
# task clear

Wipe the entire database — every task and every history event. This cannot be
undone. By default you get a confirmation prompt; pass `-y` / `-f` to skip it.

## Usage

`task clear [OPTIONS]`

## ⚠ Notice for LLM agents

`task clear` without `-y` opens a confirmation prompt and will hang. Either
pass `-y`/`-f` to confirm, or — better — don't call `clear` from an automated
flow at all (this is a destructive, irreversible operation).

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

## ⚠ Notice for LLM agents

**Don't call `task history` with no subcommand** — that opens the interactive
picker and will block. Use `task history list` (optionally with `--format md`)
to read events, and `task history --revert <ID> -y` to undo a specific one
without the confirmation prompt.

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
  edits only show the field-name tokens (`text`, `p`, `ord`, `est`) that changed.

In markdown mode the events come back as a single table with `ID`, `When`, and
`Event` columns. The `When` column always renders relative (e.g. `3d ago`).
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
        assert!(out.contains("c:a"));
        assert!(out.contains("ord:"));
        assert!(out.contains("est:"));
        assert!(!out.contains("due:"), "should not mention due: {out}");
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

    #[test]
    fn render_open_describes_links_and_picker() {
        let out = render(&["open".into()]);
        assert!(out.starts_with("# task open"));
        assert!(out.contains("picker"));
        assert!(out.contains("link number"));
    }

    #[test]
    fn extract_canonicalizes_open_short_alias() {
        let path = extract_subcommand_path(&args(&["o", "--help", "--format", "md"]));
        assert_eq!(path, vec!["open".to_string()]);
    }
}
